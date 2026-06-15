//! `delete` — remove a key from an agent's soul.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The soul key to delete.
    #[arg(long)]
    pub key: String,
    /// Full id of the agent whose soul to delete from. Defaults to the
    /// configured `OBJECTIVEAI_AGENT_FULL_ID` (the caller's own) when omitted.
    #[arg(long)]
    pub agent_full_id: Option<String>,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = ctx.agent_full_id(self.agent_full_id)?;
        let db = ctx.db().await?;
        // `true` if a key was actually removed, `false` if it didn't exist.
        let existed = db.delete_key(&agent, &self.key).await?;
        Ok(serde_json::Value::Bool(existed))
    }
}
