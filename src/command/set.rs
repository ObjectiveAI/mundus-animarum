//! `set` — create or overwrite a key in an agent's soul.

use clap::Args as ClapArgs;

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
    /// Full id of the agent whose soul to write. Defaults to the configured
    /// `OBJECTIVEAI_AGENT_FULL_ID` (the caller's own) when omitted.
    #[arg(long)]
    pub agent_full_id: Option<String>,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let agent = ctx.agent_full_id(self.agent_full_id)?;
        let db = ctx.db().await?;
        db.set_key(&agent, &self.key, &self.value).await?;
        // Echo the stored value back, mirroring `get`.
        Ok(serde_json::Value::String(self.value))
    }
}
