//! `daemon` subcommand group.
//!
//! `daemon begin` *is* the MCP server: the objectiveai host spawns it (per the
//! plugin manifest's `daemon: true`) as the single shared daemon for a state.
//! It binds a loopback port, publishes the connect URL to the `"mcp"` lockfile,
//! and serves until the process exits. The `mcp mundus-animarum begin` launcher
//! reads that URL and announces it to the host.

use clap::Subcommand;

use crate::context::Context;
use crate::error::Error;

pub mod begin;

#[derive(Subcommand)]
pub enum Commands {
    /// Launch the MCP server (the shared daemon for this state).
    Begin(begin::Args),
}

impl Commands {
    pub(crate) async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        match self {
            Commands::Begin(args) => args.run(ctx).await,
        }
    }
}
