//! `mcp` subcommand group.
//!
//! `mcp mundus-animarum begin` turns this process into the mundus-animarum
//! MCP server. The server is nested under its name so the objectiveai host's
//! `mcp <name> begin` launch convention resolves here.

use clap::Subcommand;

use crate::context::Context;
use crate::error::Error;

pub mod mundus_animarum;

#[derive(Subcommand)]
pub enum Commands {
    /// The mundus-animarum MCP server.
    #[command(name = "mundus-animarum")]
    MundusAnimarum {
        #[command(subcommand)]
        command: mundus_animarum::Commands,
    },
}

impl Commands {
    pub(crate) async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        match self {
            Commands::MundusAnimarum { command } => command.run(ctx).await,
        }
    }
}
