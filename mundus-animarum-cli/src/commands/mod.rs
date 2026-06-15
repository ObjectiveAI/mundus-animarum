//! CLI command surface.
//!
//! Each subcommand lives in its own folder module (`commands::<name>`)
//! holding its clap args and handler. The top-level entry point is
//! [`run`], invoked from `main.rs` with the raw argv: it parses via clap
//! and dispatches to the selected handler, returning the command's JSON
//! result (or an [`Error`](crate::error::Error)). `main` owns all output.

use std::ffi::OsString;

use clap::{Parser, Subcommand};
use serde_json::Value;

use crate::context::Context;
use crate::error::Error;

pub mod get;

#[derive(Parser)]
#[command(name = "mundus-animarum")]
#[command(about = "Command-line interface for the world of ObjectiveAI agent souls")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Read the value of a key in an agent's soul.
    Get(get::Args),
}

impl Commands {
    async fn handle(self, ctx: &Context) -> Result<Value, Error> {
        match self {
            Commands::Get(args) => args.run(ctx).await,
        }
    }
}

/// Help and version are informational clap "errors" — they carry the text
/// the user asked for and should be printed and exit 0, not treated as
/// parse failures. `main` uses this to special-case [`Error::Clap`].
pub fn is_informational(e: &clap::Error) -> bool {
    use clap::error::ErrorKind;
    matches!(
        e.kind(),
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayVersion
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    )
}

/// Parse `args` and dispatch to the matching handler, returning the
/// command's JSON result. Parsing failures (and `--help` / `--version`)
/// surface as [`Error::Clap`]. `main` renders both the `Ok` and `Err`
/// outcomes to stdout.
pub async fn run<I, T>(args: I, ctx: &Context) -> Result<Value, Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;
    cli.command.handle(ctx).await
}
