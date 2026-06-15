//! `get` — read the value of a single key from an agent's soul.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The soul key to read.
    #[arg(long)]
    pub key: String,
    /// Full id of the agent whose soul to read.
    #[arg(long)]
    pub agent_full_id: String,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let db = ctx.db().await?;
        let value = db
            .get_key(&self.agent_full_id, &self.agent_full_id, &self.key)
            .await?;
        Ok(value.map_or(serde_json::Value::Null, serde_json::Value::String))
    }
}
