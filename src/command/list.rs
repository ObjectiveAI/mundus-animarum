//! `list` — list every key in an agent's soul.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Full id of the agent whose soul to list. Defaults to the configured
    /// `OBJECTIVEAI_AGENT_FULL_ID` (the caller's own) when omitted.
    #[arg(long)]
    pub agent_full_id: Option<String>,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = ctx.agent_full_id(self.agent_full_id)?;
        let db = ctx.db().await?;
        // A CLI listing does NOT resolve notifications (`reader = None`): only
        // an agent's own MCP read clears its subscription.
        let keys = db.list_keys(None, &agent).await?;
        Ok(serde_json::Value::Array(
            keys.into_iter().map(serde_json::Value::String).collect(),
        ))
    }
}
