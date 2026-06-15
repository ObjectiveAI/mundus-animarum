//! Per-process context threaded as `&Context` through every command
//! handler. Holds the env-derived [`Config`](crate::run::Config) and a
//! lazily-connected soul store ([`Db`]).
//!
//! The `Db` is built on first use rather than at startup so commands that
//! never touch postgres (e.g. `--help`) don't pay the connection cost and
//! don't fail when the database is unreachable.

use mundus_animarum_db::Db;
use tokio::sync::OnceCell;

use crate::error::Error;

pub struct Context {
    /// The env-derived runtime config.
    pub config: crate::run::Config,
    /// The soul store, connected lazily on first [`db`](Context::db) call
    /// and cached thereafter. Private so the only access path is the async
    /// accessor, which guarantees a single connect.
    db: OnceCell<Db>,
}

impl Context {
    /// Build the context from the process environment. Loads the config
    /// (panics on missing required vars, see [`crate::run::load_config`])
    /// but does not connect to postgres — that happens on first
    /// [`db`](Context::db) call.
    pub fn new() -> Self {
        Self {
            config: crate::run::load_config(),
            db: OnceCell::new(),
        }
    }

    /// The soul store, connecting (and applying the schema) on first call
    /// and returning the cached handle on every call after. Fails if the
    /// connection can't be established (bad URL, server down).
    pub async fn db(&self) -> Result<&Db, Error> {
        self.db
            .get_or_try_init(|| async {
                Db::connect(&self.config.postgres_url)
                    .await
                    .map_err(Error::from)
            })
            .await
    }

    /// The calling agent's full id, from `OBJECTIVEAI_AGENT_FULL_ID` — i.e.
    /// "self", the agent the CLI runs as. Required by the subscription and
    /// notification commands; errors when unset.
    pub fn caller(&self) -> Result<&str, Error> {
        self.config
            .objectiveai_agent_full_id
            .as_deref()
            .ok_or(Error::AgentFullIdRequired)
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
