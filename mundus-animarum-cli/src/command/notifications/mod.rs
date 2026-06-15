//! `notifications` — list the caller's pending soul-change notifications.
//!
//! Takes no arguments: the subscriber is always the caller's instance
//! hierarchy. Read-only — it does not clear anything.

use clap::Args as ClapArgs;
use mundus_animarum_db::Scope;

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let caller = ctx.caller();
        let db = ctx.db().await?;
        let notifications = db.notifications(caller).await?;
        // One JSON object per pending notification: a single-key change
        // carries `key`; a whole-key-set change carries `keys: true`.
        let items = notifications
            .into_iter()
            .map(|n| match n.scope {
                Scope::Key(key) => serde_json::json!({ "target": n.target, "key": key }),
                Scope::Soul => serde_json::json!({ "target": n.target, "keys": true }),
            })
            .collect();
        Ok(serde_json::Value::Array(items))
    }
}
