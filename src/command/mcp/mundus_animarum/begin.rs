//! `mcp mundus-animarum begin` — bind 127.0.0.1:0, announce the URL (the
//! objectiveai `cli::plugins::Output::Mcp` shape, constructed and printed by
//! [`crate::mcp::run`]), and serve until the process is killed.
//!
//! Takes no arguments — per-session identity flows in from the
//! `X-OBJECTIVEAI-*` request headers.

use clap::Args as ClapArgs;

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {}

impl Args {
    pub(crate) async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        let db = ctx.db().await?.clone();
        crate::mcp::run("127.0.0.1", 0, db)
            .await
            .map_err(|e| Error::Other(format!("mcp server: {e}")))?;
        // `run` only returns once the listener stops accepting — i.e. the
        // process is being torn down.
        Ok(serde_json::Value::Null)
    }
}
