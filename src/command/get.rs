//! `get` — read the value of a single key from an agent's soul.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The soul key to read.
    #[arg(long)]
    pub key: String,
    /// Full id of the agent whose soul to read. Defaults to the configured
    /// `OBJECTIVEAI_AGENT_FULL_ID` (the caller's own) when omitted.
    #[arg(long)]
    pub agent_full_id: Option<String>,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = ctx.agent_full_id(self.agent_full_id)?;
        let db = ctx.db().await?;
        // A CLI read does NOT resolve notifications (`reader = None`): only an
        // agent's own MCP read clears its subscription. An operator inspecting
        // a soul from the CLI must not resolve that agent's notifications.
        let value = db.get_key(None, &agent, &self.key).await?;
        Ok(value.map_or(serde_json::Value::Null, serde_json::Value::String))
    }
}
