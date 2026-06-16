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
        // The reader (whose subscription this read clears) is the caller's
        // instance hierarchy — subscriptions are owned by it, not the full id.
        let value = db.get_key(ctx.caller(), &agent, &self.key).await?;
        Ok(value.map_or(serde_json::Value::Null, serde_json::Value::String))
    }
}
