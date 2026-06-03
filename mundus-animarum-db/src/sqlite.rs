//! SQLite-backed [`Database`] implementation, via `sqlx`.

use crate::Database;
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

    /// Create the `souls` table if it doesn't already exist.
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

        Ok(())
    }
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
}
