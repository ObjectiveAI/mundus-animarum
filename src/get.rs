//! `get` — read the value of a single key from an agent's soul.

use clap::Args as ClapArgs;

use crate::agent_ref::AgentRef;
use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The soul key to read.
    #[arg(long)]
    pub key: String,
    #[command(flatten)]
    pub agent: AgentRef,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = self.agent.resolve(&ctx.config)?;
        let db = ctx.db().await?;
        // Reading your own soul: reader and target are the same agent.
        // The value is returned as a JSON string; an unset key is null.
        let value = db.get_key(&agent, &agent, &self.key).await?;
        Ok(value.map_or(serde_json::Value::Null, serde_json::Value::String))
    }
}
