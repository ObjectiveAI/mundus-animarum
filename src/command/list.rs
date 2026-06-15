//! `list` — list every key in an agent's soul.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Full id of the agent whose soul to list.
    #[arg(long)]
    pub agent_full_id: String,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let db = ctx.db().await?;
        let keys = db
            .list_keys(&self.agent_full_id, &self.agent_full_id)
            .await?;
        Ok(serde_json::Value::Array(
            keys.into_iter().map(serde_json::Value::String).collect(),
        ))
    }
}
