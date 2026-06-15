//! `list` — list every key in an agent's soul.

use clap::Args as ClapArgs;

use crate::command::agent_ref::AgentRef;
use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(flatten)]
    pub agent: AgentRef,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = self.agent.resolve(&ctx.config)?;
        let db = ctx.db().await?;
        // Listing your own soul: reader and target are the same agent.
        let keys = db.list_keys(&agent, &agent).await?;
        Ok(serde_json::Value::Array(
            keys.into_iter().map(serde_json::Value::String).collect(),
        ))
    }
}
