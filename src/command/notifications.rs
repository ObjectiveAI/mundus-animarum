//! `notifications` — list a subscriber's pending soul-change notifications.
//!
//! The subscriber defaults to the configured instance hierarchy, narrowed by
//! the optional `--agent-instance` / `--parent-agent-instance-hierarchy`
//! selector. Read-only — it does not clear anything.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::db::Scope;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Read notifications for `<configured AIH>/<agent_instance>` (or
    /// `<parent>/<agent_instance>` when `--parent-agent-instance-hierarchy`
    /// is given). Omitted ⇒ the configured instance hierarchy itself.
    #[arg(long)]
    pub agent_instance: Option<String>,
    /// Explicit parent hierarchy for `--agent-instance`. Only valid alongside
    /// it.
    #[arg(long, requires = "agent_instance")]
    pub parent_agent_instance_hierarchy: Option<String>,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let subscriber = ctx.agent_instance_hierarchy(
            self.agent_instance,
            self.parent_agent_instance_hierarchy,
        );
        let db = ctx.db().await?;
        let notifications = db.notifications(&subscriber).await?;
        // One JSON object per pending notification: a single-key change
        // carries `key`; a whole-soul (key-set) change carries `soul: true`.
        let items = notifications
            .into_iter()
            .map(|n| match n.scope {
                Scope::Key(key) => serde_json::json!({ "target": n.target, "key": key }),
                Scope::Soul => serde_json::json!({ "target": n.target, "soul": true }),
            })
            .collect();
        Ok(serde_json::Value::Array(items))
    }
}
