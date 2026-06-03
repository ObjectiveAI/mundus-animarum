//! SQLite-backed [`Database`] implementation, via `sqlx`.

use crate::{Database, Remark};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
            "CREATE TABLE IF NOT EXISTS remarks (\
                id      INTEGER PRIMARY KEY AUTOINCREMENT, \
                target  TEXT NOT NULL, \
                key     TEXT NOT NULL, \
                author  TEXT NOT NULL, \
                body    TEXT NOT NULL, \
                created INTEGER NOT NULL, \
                read    INTEGER NOT NULL DEFAULT 0)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS remarks_target_key ON remarks (target, key, id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS remarks_unread ON remarks (target, read)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

/// Current Unix time in whole seconds (saturating to 0 before the epoch).
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Database for Sqlite {
    type Error = sqlx::Error;

    async fn list_keys(&self, agent: &str) -> Result<Vec<String>, Self::Error> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT key FROM souls WHERE agent = ? ORDER BY key")
                .bind(agent)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(k,)| k).collect())
    }

    async fn get_key(&self, agent: &str, key: &str) -> Result<Option<String>, Self::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM souls WHERE agent = ? AND key = ?")
                .bind(agent)
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(v,)| v))
    }

    async fn set_key(&self, agent: &str, key: &str, value: &str) -> Result<(), Self::Error> {
        sqlx::query(
            "INSERT INTO souls (agent, key, value) VALUES (?, ?, ?) \
             ON CONFLICT(agent, key) DO UPDATE SET value = excluded.value",
        )
        .bind(agent)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_key(&self, agent: &str, key: &str) -> Result<bool, Self::Error> {
        let res = sqlx::query("DELETE FROM souls WHERE agent = ? AND key = ?")
            .bind(agent)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    async fn add_remark(
        &self,
        author: &str,
        target: &str,
        key: &str,
        body: &str,
    ) -> Result<(), Self::Error> {
        sqlx::query(
            "INSERT INTO remarks (target, key, author, body, created, read) \
             VALUES (?, ?, ?, ?, ?, 0)",
        )
        .bind(target)
        .bind(key)
        .bind(author)
        .bind(body)
        .bind(now_secs())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_remarks(
        &self,
        target: &str,
        key: &str,
        offset: u64,
        count: u32,
        unread_only: bool,
    ) -> Result<Vec<Remark>, Self::Error> {
        let sql = if unread_only {
            "SELECT id, author, body, created, read FROM remarks \
             WHERE target = ? AND key = ? AND read = 0 ORDER BY id ASC LIMIT ? OFFSET ?"
        } else {
            "SELECT id, author, body, created, read FROM remarks \
             WHERE target = ? AND key = ? ORDER BY id ASC LIMIT ? OFFSET ?"
        };

        let mut tx = self.pool.begin().await?;

        // `read` here is the state *before* this fetch marks the rows read,
        // which is exactly what `Remark::read` documents. `u64` can't be bound
        // in sqlx-sqlite, so cast `offset`/`count` to `i64`.
        let rows: Vec<(i64, String, String, i64, bool)> = sqlx::query_as(sql)
            .bind(target)
            .bind(key)
            .bind(count as i64)
            .bind(offset as i64)
            .fetch_all(&mut *tx)
            .await?;

        if !rows.is_empty() {
            // Mark exactly the rows we're returning as read. `QueryBuilder`
            // builds the `IN (?, ?, ...)` placeholder list and binds each id,
            // satisfying sqlx 0.9's static-SQL injection guard.
            let mut qb = sqlx::QueryBuilder::new("UPDATE remarks SET read = 1 WHERE id IN (");
            {
                let mut sep = qb.separated(", ");
                for (id, ..) in &rows {
                    sep.push_bind(*id);
                }
                sep.push_unseparated(")");
            }
            qb.build().execute(&mut *tx).await?;
        }

        tx.commit().await?;

        Ok(rows
            .into_iter()
            .map(|(_, author, body, created, read)| Remark {
                author,
                body,
                created: created as u64,
                read,
            })
            .collect())
    }

    async fn unread_remarks(&self, target: &str) -> Result<Vec<(String, u64)>, Self::Error> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT key, COUNT(*) FROM remarks WHERE target = ? AND read = 0 GROUP BY key",
        )
        .bind(target)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(k, c)| (k, c as u64)).collect())
    }
}
