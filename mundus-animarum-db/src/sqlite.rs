//! SQLite-backed [`Database`] implementation, via `sqlx`.

use crate::{Database, Notification, Scope};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::path::Path;

/// SQLite-backed [`Database`].
///
/// Cheap to clone — the inner [`SqlitePool`] is an `Arc`-backed handle, so
/// clones share the same connection pool.
#[derive(Clone, Debug)]
pub struct Sqlite {
    pool: SqlitePool,
}

impl Sqlite {
    /// Open the database at `path`, creating the file (and any missing parent
    /// directories) if absent, and ensure the schema exists.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, sqlx::Error> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let db = Self {
            pool: SqlitePool::connect_with(opts).await?,
        };
        db.migrate().await?;
        Ok(db)
    }

    /// Adopt an existing pool (caller controls the connection options) and
    /// ensure the schema exists.
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        let db = Self { pool };
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
                unread     INTEGER NOT NULL DEFAULT 0, \
                PRIMARY KEY (subscriber, target, key))",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS soul_subscriptions (\
                subscriber TEXT NOT NULL, \
                target     TEXT NOT NULL, \
                unread     INTEGER NOT NULL DEFAULT 0, \
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
}

impl Database for Sqlite {
    type Error = sqlx::Error;

    async fn list_keys(&self, reader: &str, target: &str) -> Result<Vec<String>, Self::Error> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT key FROM souls WHERE agent = ? ORDER BY key")
                .bind(target)
                .fetch_all(&self.pool)
                .await?;

        // Reading the key set clears the reader's soul subscription, if any.
        sqlx::query("UPDATE soul_subscriptions SET unread = 0 WHERE subscriber = ? AND target = ?")
            .bind(reader)
            .bind(target)
            .execute(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|(k,)| k).collect())
    }

    async fn get_key(
        &self,
        reader: &str,
        target: &str,
        key: &str,
    ) -> Result<Option<String>, Self::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM souls WHERE agent = ? AND key = ?")
                .bind(target)
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        // Reading the value clears the reader's key subscription, if any —
        // regardless of whether the value still exists (a deletion-then-read
        // still clears the pending notification).
        sqlx::query(
            "UPDATE key_subscriptions SET unread = 0 \
             WHERE subscriber = ? AND target = ? AND key = ?",
        )
        .bind(reader)
        .bind(target)
        .bind(key)
        .execute(&self.pool)
        .await?;

        Ok(row.map(|(v,)| v))
    }

    async fn set_key(&self, owner: &str, key: &str, value: &str) -> Result<(), Self::Error> {
        let mut tx = self.pool.begin().await?;

        // Detect whether this is a new key (an addition to the key set).
        let existed: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM souls WHERE agent = ? AND key = ?")
                .bind(owner)
                .bind(key)
                .fetch_optional(&mut *tx)
                .await?;

        sqlx::query(
            "INSERT INTO souls (agent, key, value) VALUES (?, ?, ?) \
             ON CONFLICT(agent, key) DO UPDATE SET value = excluded.value",
        )
        .bind(owner)
        .bind(key)
        .bind(value)
        .execute(&mut *tx)
        .await?;

        // Value changed (or key created): fire key subscriptions on this key.
        sqlx::query("UPDATE key_subscriptions SET unread = 1 WHERE target = ? AND key = ?")
            .bind(owner)
            .bind(key)
            .execute(&mut *tx)
            .await?;

        // New key: the key set grew, so fire soul subscriptions too.
        if existed.is_none() {
            sqlx::query("UPDATE soul_subscriptions SET unread = 1 WHERE target = ?")
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn delete_key(&self, owner: &str, key: &str) -> Result<bool, Self::Error> {
        let mut tx = self.pool.begin().await?;

        let res = sqlx::query("DELETE FROM souls WHERE agent = ? AND key = ?")
            .bind(owner)
            .bind(key)
            .execute(&mut *tx)
            .await?;
        let existed = res.rows_affected() > 0;

        if existed {
            // Deletion fires the key subscriptions, and shrinking the key set
            // fires the soul subscriptions.
            sqlx::query("UPDATE key_subscriptions SET unread = 1 WHERE target = ? AND key = ?")
                .bind(owner)
                .bind(key)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE soul_subscriptions SET unread = 1 WHERE target = ?")
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(existed)
    }

    async fn subscribe_key(
        &self,
        subscriber: &str,
        target: &str,
        key: &str,
    ) -> Result<(), Self::Error> {
        // Start caught-up (unread = 0); a re-subscribe must not reset an
        // existing pending notification, so DO NOTHING on conflict.
        sqlx::query(
            "INSERT INTO key_subscriptions (subscriber, target, key, unread) \
             VALUES (?, ?, ?, 0) ON CONFLICT(subscriber, target, key) DO NOTHING",
        )
        .bind(subscriber)
        .bind(target)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn subscribe_soul(&self, subscriber: &str, target: &str) -> Result<(), Self::Error> {
        sqlx::query(
            "INSERT INTO soul_subscriptions (subscriber, target, unread) \
             VALUES (?, ?, 0) ON CONFLICT(subscriber, target) DO NOTHING",
        )
        .bind(subscriber)
        .bind(target)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn unsubscribe_key(
        &self,
        subscriber: &str,
        target: &str,
        key: &str,
    ) -> Result<(), Self::Error> {
        sqlx::query(
            "DELETE FROM key_subscriptions WHERE subscriber = ? AND target = ? AND key = ?",
        )
        .bind(subscriber)
        .bind(target)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn unsubscribe_soul(&self, subscriber: &str, target: &str) -> Result<(), Self::Error> {
        sqlx::query("DELETE FROM soul_subscriptions WHERE subscriber = ? AND target = ?")
            .bind(subscriber)
            .bind(target)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn notifications(&self, subscriber: &str) -> Result<Vec<Notification>, Self::Error> {
        let key_rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT target, key FROM key_subscriptions \
             WHERE subscriber = ? AND unread = 1 ORDER BY target, key",
        )
        .bind(subscriber)
        .fetch_all(&self.pool)
        .await?;

        let soul_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT target FROM soul_subscriptions \
             WHERE subscriber = ? AND unread = 1 ORDER BY target",
        )
        .bind(subscriber)
        .fetch_all(&self.pool)
        .await?;

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
        Ok(out)
    }
}
