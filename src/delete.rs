//! `delete` — remove a key from an agent's soul.

use clap::Args as ClapArgs;

use crate::agent_ref::AgentRef;
use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The soul key to delete.
    #[arg(long)]
    pub key: String,
    #[command(flatten)]
    pub agent: AgentRef,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = self.agent.resolve(&ctx.config)?;
        let db = ctx.db().await?;
        // `true` if a key was actually removed, `false` if it didn't exist.
        let existed = db.delete_key(&agent, &self.key).await?;
        Ok(serde_json::Value::Bool(existed))
    }
}
