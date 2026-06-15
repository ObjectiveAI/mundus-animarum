//! CLI command surface.
//!
//! Each subcommand lives in its own folder module (`commands::<name>`)
//! holding its clap args and handler. [`Cli`] is the clap root; the
//! top-level [`run`](crate::run) entry point (in `run.rs`) parses argv into
//! it and dispatches through [`Commands::handle`].

use clap::{Parser, Subcommand};
use serde_json::Value;

use crate::context::Context;
use crate::error::Error;

pub mod get;

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
}

impl Commands {
    pub(crate) async fn handle(self, ctx: &Context) -> Result<Value, Error> {
        match self {
            Commands::Get(args) => args.run(ctx).await,
        }
    }
}
