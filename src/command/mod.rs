//! CLI command surface.
//!
//! Each subcommand lives in its own module under `command`; [`Cli`] is the
//! clap root. The top-level [`run`](crate::run) entry point (in `run.rs`)
//! parses argv into it and dispatches through [`Commands::handle`].

use clap::{Parser, Subcommand};
use serde_json::Value;

use crate::context::Context;
use crate::error::Error;

pub(crate) mod subscription;

pub mod daemon;
pub mod delete;
pub mod get;
pub mod list;
pub mod mcp;
pub mod set;
pub mod subscribe;
pub mod subscriptions;
pub mod unsubscribe;

#[derive(Parser)]
#[command(name = "mundus-animarum")]
#[command(about = "Command-line interface for the world of ObjectiveAI agent souls")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Read the value of a key in an agent's soul.
    Get(get::Args),
    /// Create or overwrite a key in an agent's soul.
    Set(set::Args),
    /// List every key in an agent's soul.
    List(list::Args),
    /// Delete a key from an agent's soul.
    Delete(delete::Args),
    /// Watch another agent's soul (a single key or the whole key set).
    Subscribe(subscribe::Args),
    /// Stop watching another agent's soul.
    Unsubscribe(unsubscribe::Args),
    /// List the subscriptions owned by an entity.
    Subscriptions(subscriptions::Args),
    /// MCP server management (`mcp mundus-animarum begin`).
    Mcp {
        #[command(subcommand)]
        command: mcp::Commands,
    },
    /// Daemon management (`daemon begin` — the shared MCP server).
    Daemon {
        #[command(subcommand)]
        command: daemon::Commands,
    },
}

impl Commands {
    pub(crate) async fn handle(self, ctx: &Context) -> Result<Value, Error> {
        match self {
            Commands::Get(args) => args.run(ctx).await,
            Commands::Set(args) => args.run(ctx).await,
            Commands::List(args) => args.run(ctx).await,
            Commands::Delete(args) => args.run(ctx).await,
            Commands::Subscribe(args) => args.run(ctx).await,
            Commands::Unsubscribe(args) => args.run(ctx).await,
            Commands::Subscriptions(args) => args.run(ctx).await,
            Commands::Mcp { command } => command.run(ctx).await,
            Commands::Daemon { command } => command.run(ctx).await,
        }
    }
}
