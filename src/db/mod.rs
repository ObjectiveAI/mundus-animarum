//! mundus-animarum database layer — Postgres-backed storage for ObjectiveAI
//! agent souls, via `sqlx`.

use sqlx::PgPool;

/// The error type returned by every [`Db`] operation — re-exported from
/// `sqlx` so callers don't need a direct `sqlx` dependency.
pub use sqlx::Error;

/// What a subscription / notification is scoped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// A single soul key — value changes and deletion of that key.
    Key(String),
    /// The target's whole key set — key additions and deletions.
    Soul,
}

/// A pending change notification for a subscriber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The agent whose soul changed.
    pub target: String,
    /// What changed.
    pub scope: Scope,
}

/// A subscription held by a subscriber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    /// The agent whose soul is watched.
    pub target: String,
    /// What is watched — a single key, or the whole key set.
    pub scope: Scope,
}

/// Postgres-backed storage for agent souls and soul-change subscriptions.
///
/// A *soul* is a mutable, self-authored `key → value` store bound to an
/// ObjectiveAI agent's immutable, content-addressed ID (a 22-character base62
/// string).
///
/// Agents can **subscribe** to another agent's soul — either to a single key
/// (value changes and deletion of that key) or to the whole key set (key
/// additions and deletions). Subscriptions form a coalesced notification queue:
/// at most one pending notification per subscription, cleared either by reading
/// the subscribed data (reading a key clears that key's notification; listing
/// the keys clears the soul notification) or by claiming it from the
/// notification queue (see [`take_notifications`](Db::take_notifications)).
///
/// This type is **identity-agnostic**: it has no notion of a "current" or
/// "self" agent. Every operation names the agent(s) it acts on by explicit ID.
/// Deciding whether a call targets the caller's own soul or another agent's —
/// and supplying the caller's ID — is the responsibility of the layer above
/// (the CLI), not this type.
///
/// Cheap to clone — the inner [`PgPool`] is an `Arc`-backed handle, so clones
/// share the same connection pool.
#[derive(Clone, Debug)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// Connect to the Postgres database at `url` and ensure the schema exists.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let db = Self {
            pool: PgPool::connect(url).await?,
        };
        db.migrate().await?;
        Ok(db)
    }

    /// Create the tables and indexes if they don't already exist.
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS souls (\
                agent TEXT NOT NULL, \
                key   TEXT NOT NULL, \
                value TEXT NOT NULL, \
                PRIMARY KEY (agent, key))",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS key_subscriptions (\
                subscriber TEXT NOT NULL, \
                target     TEXT NOT NULL, \
                key        TEXT NOT NULL, \
                unread     BOOLEAN NOT NULL DEFAULT FALSE, \
                PRIMARY KEY (subscriber, target, key))",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS soul_subscriptions (\
                subscriber TEXT NOT NULL, \
                target     TEXT NOT NULL, \
                unread     BOOLEAN NOT NULL DEFAULT FALSE, \
                PRIMARY KEY (subscriber, target))",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS key_subscriptions_target \
             ON key_subscriptions (target, key)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS soul_subscriptions_target \
             ON soul_subscriptions (target)",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ---- Soul keys ----

    /// List every soul key owned by `target`.
    ///
    /// `reader = Some(r)`: clears `r`'s soul subscription on `target`, if any
    /// (marks the pending notification read). `reader = None`: a read-only
    /// listing that resolves nothing — the CLI passes this so an operator
    /// inspecting a soul never resolves an agent's notifications (only the
    /// agent's own MCP reads do).
    pub async fn list_keys(
        &self,
        reader: Option<&str>,
        target: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT key FROM souls WHERE agent = $1 ORDER BY key")
                .bind(target)
                .fetch_all(&self.pool)
                .await?;

        // Reading the key set clears the reader's soul subscription, if any.
        if let Some(reader) = reader {
            sqlx::query(
                "UPDATE soul_subscriptions SET unread = FALSE WHERE subscriber = $1 AND target = $2",
            )
            .bind(reader)
            .bind(target)
            .execute(&self.pool)
            .await?;
        }

        Ok(rows.into_iter().map(|(k,)| k).collect())
    }

    /// Retrieve the value of `target`'s soul `key`, or `None` if unset.
    ///
    /// `reader = Some(r)`: clears `r`'s key subscription on `(target, key)`,
    /// if any (marks the pending notification read) — regardless of whether
    /// the value still exists (a deletion-then-read still clears it).
    /// `reader = None`: a read-only get that resolves nothing — the CLI passes
    /// this so inspecting a soul never resolves an agent's notifications (only
    /// the agent's own MCP reads do).
    pub async fn get_key(
        &self,
        reader: Option<&str>,
        target: &str,
        key: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM souls WHERE agent = $1 AND key = $2")
                .bind(target)
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        if let Some(reader) = reader {
            sqlx::query(
                "UPDATE key_subscriptions SET unread = FALSE \
                 WHERE subscriber = $1 AND target = $2 AND key = $3",
            )
            .bind(reader)
            .bind(target)
            .bind(key)
            .execute(&self.pool)
            .await?;
        }

        Ok(row.map(|(v,)| v))
    }

    /// Create or overwrite `owner`'s soul `key` with `value`.
    ///
    /// Fires the key subscriptions on `(owner, key)`; if the key is new, also
    /// fires the soul subscriptions on `owner` (the key set grew).
    pub async fn set_key(&self, owner: &str, key: &str, value: &str) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Upsert and learn, in one statement, whether a new key was created: a
        // freshly inserted row has `xmax = 0`, while an overwritten one carries
        // this transaction's id. Detecting it atomically (rather than a separate
        // SELECT-then-INSERT) avoids a check-then-write race that could drop a
        // soul-subscription notification.
        let (inserted,): (bool,) = sqlx::query_as(
            "INSERT INTO souls (agent, key, value) VALUES ($1, $2, $3) \
             ON CONFLICT (agent, key) DO UPDATE SET value = EXCLUDED.value \
             RETURNING (xmax = '0'::xid)",
        )
        .bind(owner)
        .bind(key)
        .bind(value)
        .fetch_one(&mut *tx)
        .await?;

        // Value changed (or key created): fire key subscriptions on this key.
        sqlx::query("UPDATE key_subscriptions SET unread = TRUE WHERE target = $1 AND key = $2")
            .bind(owner)
            .bind(key)
            .execute(&mut *tx)
            .await?;

        // New key: the key set grew, so fire soul subscriptions too.
        if inserted {
            sqlx::query("UPDATE soul_subscriptions SET unread = TRUE WHERE target = $1")
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Delete `owner`'s soul `key`. Returns `true` if a key was removed,
    /// `false` if it did not exist.
    ///
    /// If a key was removed, fires the key subscriptions on `(owner, key)` and
    /// the soul subscriptions on `owner` (the key set shrank).
    pub async fn delete_key(&self, owner: &str, key: &str) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let res = sqlx::query("DELETE FROM souls WHERE agent = $1 AND key = $2")
            .bind(owner)
            .bind(key)
            .execute(&mut *tx)
            .await?;
        let existed = res.rows_affected() > 0;

        if existed {
            // Deletion fires the key subscriptions, and shrinking the key set
            // fires the soul subscriptions.
            sqlx::query("UPDATE key_subscriptions SET unread = TRUE WHERE target = $1 AND key = $2")
                .bind(owner)
                .bind(key)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE soul_subscriptions SET unread = TRUE WHERE target = $1")
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(existed)
    }

    // ---- Subscriptions ----

    /// Subscribe `subscriber` to value changes and deletion of `target`'s
    /// soul `key`. Idempotent; starts caught-up (no pending notification).
    pub async fn subscribe_key(
        &self,
        subscriber: &str,
        target: &str,
        key: &str,
    ) -> Result<(), sqlx::Error> {
        // Start caught-up (unread = FALSE); a re-subscribe must not reset an
        // existing pending notification, so DO NOTHING on conflict.
        sqlx::query(
            "INSERT INTO key_subscriptions (subscriber, target, key, unread) \
             VALUES ($1, $2, $3, FALSE) ON CONFLICT (subscriber, target, key) DO NOTHING",
        )
        .bind(subscriber)
        .bind(target)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Subscribe `subscriber` to additions and deletions in `target`'s key
    /// set. Idempotent; starts caught-up (no pending notification).
    pub async fn subscribe_soul(&self, subscriber: &str, target: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO soul_subscriptions (subscriber, target, unread) \
             VALUES ($1, $2, FALSE) ON CONFLICT (subscriber, target) DO NOTHING",
        )
        .bind(subscriber)
        .bind(target)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove `subscriber`'s key subscription on `(target, key)`, if any.
    pub async fn unsubscribe_key(
        &self,
        subscriber: &str,
        target: &str,
        key: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM key_subscriptions WHERE subscriber = $1 AND target = $2 AND key = $3",
        )
        .bind(subscriber)
        .bind(target)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove `subscriber`'s soul subscription on `target`, if any.
    pub async fn unsubscribe_soul(&self, subscriber: &str, target: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM soul_subscriptions WHERE subscriber = $1 AND target = $2")
            .bind(subscriber)
            .bind(target)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// List every subscription owned by `subscriber` (read-only). Key
    /// subscriptions come before soul ones, each in `(target, key)` / `target`
    /// order.
    pub async fn subscriptions(
        &self,
        subscriber: &str,
    ) -> Result<Vec<Subscription>, sqlx::Error> {
        let key_rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT target, key FROM key_subscriptions \
             WHERE subscriber = $1 ORDER BY target, key",
        )
        .bind(subscriber)
        .fetch_all(&self.pool)
        .await?;

        let soul_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT target FROM soul_subscriptions WHERE subscriber = $1 ORDER BY target",
        )
        .bind(subscriber)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(key_rows.len() + soul_rows.len());
        for (target, key) in key_rows {
            out.push(Subscription {
                target,
                scope: Scope::Key(key),
            });
        }
        for (target,) in soul_rows {
            out.push(Subscription {
                target,
                scope: Scope::Soul,
            });
        }
        Ok(out)
    }

    // ---- Notification queue ----

    /// Claim up to `limit` of `subscriber`'s pending notifications: return them
    /// **and mark exactly those resolved** in the same transaction, plus a
    /// count of how many pending notifications remain.
    ///
    /// Concurrency (built for high parallelism): the whole operation is one
    /// transaction, and each batch is claimed with `FOR UPDATE SKIP LOCKED`. So
    /// any number of parallel callers — even for the same subscriber — take
    /// **disjoint** batches: no double-claim, no lost notification, and no
    /// caller blocks another (locked rows are skipped, not waited on). A
    /// concurrent write (`set_key`/`delete_key`) that re-raises a row is either
    /// seen before the claim (and may be taken) or re-fires after it — row
    /// locks serialize the two, so it is never dropped. Key notifications are
    /// claimed before soul ones, each picked in `(target, key)` / `target`
    /// order under the `LIMIT`. `remaining` is counted after the claim within
    /// the same transaction (own claims already excluded); under heavy
    /// concurrency it is an upper bound — it may include rows another in-flight
    /// claim is about to resolve — never an undercount.
    pub async fn take_notifications(
        &self,
        subscriber: &str,
        limit: u32,
    ) -> Result<(Vec<Notification>, u64), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // Claim (resolve) up to `limit` key notifications, returning exactly
        // those. SKIP LOCKED means parallel claims never collide.
        let key_rows: Vec<(String, String)> = sqlx::query_as(
            "UPDATE key_subscriptions SET unread = FALSE \
             WHERE (subscriber, target, key) IN ( \
                 SELECT subscriber, target, key FROM key_subscriptions \
                 WHERE subscriber = $1 AND unread \
                 ORDER BY target, key LIMIT $2 FOR UPDATE SKIP LOCKED \
             ) RETURNING target, key",
        )
        .bind(subscriber)
        .bind(i64::from(limit))
        .fetch_all(&mut *tx)
        .await?;

        // Fill the remaining budget with soul notifications.
        let soul_limit = i64::from(limit) - key_rows.len() as i64;
        let soul_rows: Vec<(String,)> = if soul_limit > 0 {
            sqlx::query_as(
                "UPDATE soul_subscriptions SET unread = FALSE \
                 WHERE (subscriber, target) IN ( \
                     SELECT subscriber, target FROM soul_subscriptions \
                     WHERE subscriber = $1 AND unread \
                     ORDER BY target LIMIT $2 FOR UPDATE SKIP LOCKED \
                 ) RETURNING target",
            )
            .bind(subscriber)
            .bind(soul_limit)
            .fetch_all(&mut *tx)
            .await?
        } else {
            Vec::new()
        };

        // Count what's still pending, in the same transaction — our own claims
        // are already excluded since they were set FALSE above.
        let (remaining,): (i64,) = sqlx::query_as(
            "SELECT \
                (SELECT count(*) FROM key_subscriptions WHERE subscriber = $1 AND unread) \
              + (SELECT count(*) FROM soul_subscriptions WHERE subscriber = $1 AND unread)",
        )
        .bind(subscriber)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        let mut out = Vec::with_capacity(key_rows.len() + soul_rows.len());
        for (target, key) in key_rows {
            out.push(Notification {
                target,
                scope: Scope::Key(key),
            });
        }
        for (target,) in soul_rows {
            out.push(Notification {
                target,
                scope: Scope::Soul,
            });
        }
        Ok((out, remaining as u64))
    }
}
