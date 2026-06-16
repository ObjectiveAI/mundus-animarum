//! Per-process context threaded as `&Context` through every command
//! handler. Holds the env-derived [`Config`](crate::run::Config) and a
//! lazily-connected soul store ([`Db`]).
//!
//! The `Db` is built on first use rather than at startup so commands that
//! never touch postgres (e.g. `--help`) don't pay the connection cost and
//! don't fail when the database is unreachable.

use tokio::sync::OnceCell;

use crate::db::Db;
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

    /// The subscription owner: this agent's instance hierarchy
    /// (`OBJECTIVEAI_AGENT_INSTANCE_HIERARCHY`, defaulting to
    /// "mundus-animarum"). Subscriptions and notifications are owned by the
    /// instance hierarchy rather than the agent full id — multiple agents can
    /// share a full id, so each instance hierarchy tracks its own.
    pub fn caller(&self) -> &str {
        &self.config.objectiveai_agent_instance_hierarchy
    }

    /// Resolve a target agent full id for the soul commands: the explicit
    /// `--agent-full-id` if given, otherwise the configured
    /// `OBJECTIVEAI_AGENT_FULL_ID` (the caller's own soul). Errors when
    /// neither is available.
    pub fn agent_full_id(&self, arg: Option<String>) -> Result<String, Error> {
        arg.or_else(|| self.config.objectiveai_agent_full_id.clone())
            .ok_or(Error::AgentFullIdRequired)
    }

    /// Resolve the subscription-owner AIH from the optional instance selector:
    /// - neither given: the configured AIH ([`caller`](Context::caller));
    /// - `agent_instance` only: `<configured AIH>/<agent_instance>`;
    /// - `parent` + `agent_instance`: `<parent>/<agent_instance>`.
    ///
    /// (`parent` without `agent_instance` is rejected by clap, so that case
    /// never reaches here.)
    pub fn agent_instance_hierarchy(
        &self,
        agent_instance: Option<String>,
        parent_agent_instance_hierarchy: Option<String>,
    ) -> String {
        match (agent_instance, parent_agent_instance_hierarchy) {
            (None, _) => self.caller().to_string(),
            (Some(instance), None) => format!("{}/{instance}", self.caller()),
            (Some(instance), Some(parent)) => format!("{parent}/{instance}"),
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
