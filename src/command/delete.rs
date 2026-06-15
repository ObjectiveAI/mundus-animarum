//! `delete` — remove a key from an agent's soul.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The soul key to delete.
    #[arg(long)]
    pub key: String,
    /// Full id of the agent whose soul to delete from.
    #[arg(long)]
    pub agent_full_id: String,
}

impl Args {
    pub async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let db = ctx.db().await?;
        // `true` if a key was actually removed, `false` if it didn't exist.
        let existed = db.delete_key(&self.agent_full_id, &self.key).await?;
        Ok(serde_json::Value::Bool(existed))
    }
}
