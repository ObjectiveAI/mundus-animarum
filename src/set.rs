//! `set` — create or overwrite a key in an agent's soul.

use clap::Args as ClapArgs;

use crate::agent_ref::AgentRef;
use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The soul key to write.
    #[arg(long)]
    pub key: String,
    /// The value to store.
    #[arg(long)]
    pub value: String,
    #[command(flatten)]
    pub agent: AgentRef,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = self.agent.resolve(&ctx.config)?;
        let db = ctx.db().await?;
        db.set_key(&agent, &self.key, &self.value).await?;
        // Echo the stored value back, mirroring `get`.
        Ok(serde_json::Value::String(self.value))
    }
}
