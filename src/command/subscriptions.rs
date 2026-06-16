//! `subscriptions` — list the subscriptions owned by an entity.
//!
//! The entity (subscription owner) is selected exactly like the
//! subscribe/unsubscribe owner: the configured instance hierarchy, narrowed by
//! the optional `--agent-instance` / `--parent-agent-instance-hierarchy`
//! (resolved by [`Context::agent_instance_hierarchy`](crate::context::Context::agent_instance_hierarchy)).

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::db::Scope;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// List subscriptions owned by `<configured AIH>/<agent_instance>` (or
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
        let entity = ctx
            .agent_instance_hierarchy(self.agent_instance, self.parent_agent_instance_hierarchy);
        let db = ctx.db().await?;
        let subscriptions = db.subscriptions(&entity).await?;
        // One JSON object per subscription: a key subscription carries `key`;
        // a soul (key-set) subscription carries `soul: true`.
        let items: Vec<serde_json::Value> = subscriptions
            .into_iter()
            .map(|s| match s.scope {
                Scope::Key(key) => serde_json::json!({ "target": s.target, "key": key }),
                Scope::Soul => serde_json::json!({ "target": s.target, "soul": true }),
            })
            .collect();
        Ok(serde_json::Value::Array(items))
    }
}
