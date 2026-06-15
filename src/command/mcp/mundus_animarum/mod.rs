//! `mcp mundus-animarum` subcommand group — the mundus-animarum MCP server.

use clap::Subcommand;

use crate::context::Context;
use crate::error::Error;

pub mod begin;

#[derive(Subcommand)]
pub enum Commands {
    /// Start the MCP server.
    Begin(begin::Args),
}

impl Commands {
    pub(crate) async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        match self {
            Commands::Begin(args) => args.run(ctx).await,
        }
    }
}
