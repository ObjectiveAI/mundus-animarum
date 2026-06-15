use std::ffi::OsString;
use std::path::PathBuf;

use clap::Parser;
use envconfig::Envconfig;
use serde_json::Value;

use crate::commands::Cli;
use crate::context::Context;
use crate::error::Error;

// ---------------------------------------------------------------------------
// Env-driven runtime config (3-struct pattern; mirrors objectiveai-cli)
// ---------------------------------------------------------------------------

#[derive(Envconfig)]
struct EnvConfigBuilder {
    /// Root of the CLI's filesystem state tree. All durable state lives in
    /// postgres; this is the root for any local on-disk state. Required;
    /// unwrapped at `build()` (we panic if absent).
    #[envconfig(from = "OBJECTIVEAI_STATE_DIR")]
    state_dir: Option<String>,
    /// Postgres connection URL — the single persistence layer.
    /// Required; unwrapped at `build()`.
    #[envconfig(from = "OBJECTIVEAI_POSTGRES_URL")]
    postgres_url: Option<String>,
    #[envconfig(from = "OBJECTIVEAI_AGENT_ID")]
    objectiveai_agent_id: Option<String>,
    #[envconfig(from = "OBJECTIVEAI_AGENT_FULL_ID")]
    objectiveai_agent_full_id: Option<String>,
    #[envconfig(from = "OBJECTIVEAI_AGENT_REMOTE")]
    objectiveai_agent_remote: Option<String>,
    #[envconfig(from = "OBJECTIVEAI_AGENT_INSTANCE_HIERARCHY")]
    objectiveai_agent_instance_hierarchy: Option<String>,
}

impl EnvConfigBuilder {
    pub fn build(self) -> ConfigBuilder {
        ConfigBuilder {
            state_dir: self.state_dir,
            postgres_url: self.postgres_url,
            objectiveai_agent_id: self.objectiveai_agent_id,
            objectiveai_agent_full_id: self.objectiveai_agent_full_id,
            objectiveai_agent_remote: self.objectiveai_agent_remote,
            objectiveai_agent_instance_hierarchy: self.objectiveai_agent_instance_hierarchy,
        }
    }
}

#[derive(Default)]
pub struct ConfigBuilder {
    pub state_dir: Option<String>,
    pub postgres_url: Option<String>,
    pub objectiveai_agent_id: Option<String>,
    pub objectiveai_agent_full_id: Option<String>,
    pub objectiveai_agent_remote: Option<String>,
    pub objectiveai_agent_instance_hierarchy: Option<String>,
}

impl Envconfig for ConfigBuilder {
    #[allow(deprecated)]
    fn init() -> Result<Self, envconfig::Error> {
        EnvConfigBuilder::init().map(|e| e.build())
    }

    fn init_from_env() -> Result<Self, envconfig::Error> {
        EnvConfigBuilder::init_from_env().map(|e| e.build())
    }

    fn init_from_hashmap(
        h: &std::collections::HashMap<String, String>,
    ) -> Result<Self, envconfig::Error> {
        EnvConfigBuilder::init_from_hashmap(h).map(|e| e.build())
    }
}

impl ConfigBuilder {
    pub fn build(self) -> Config {
        Config {
            // Required — unwrapped here, after env init. Absence is a
            // hard misconfiguration: panic with a clear message.
            state_dir: PathBuf::from(
                self.state_dir
                    .expect("OBJECTIVEAI_STATE_DIR must be set (the state root)"),
            ),
            postgres_url: self
                .postgres_url
                .expect("OBJECTIVEAI_POSTGRES_URL must be set"),
            objectiveai_agent_id: self.objectiveai_agent_id,
            objectiveai_agent_full_id: self.objectiveai_agent_full_id,
            objectiveai_agent_remote: self.objectiveai_agent_remote,
            objectiveai_agent_instance_hierarchy: self
                .objectiveai_agent_instance_hierarchy
                .unwrap_or_else(|| "mundus-animarum".to_string()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Root of the CLI's filesystem state tree (env `OBJECTIVEAI_STATE_DIR`).
    /// All durable state lives in postgres; this is the root for any local
    /// on-disk state. Assumed to already exist. Required (panics if unset).
    pub state_dir: PathBuf,
    /// Postgres connection URL (env `OBJECTIVEAI_POSTGRES_URL`) — the
    /// single persistence layer. Required.
    pub postgres_url: String,
    /// Default agent id (env `OBJECTIVEAI_AGENT_ID`). Currently unused —
    /// captured for parity with the objectiveai agent-environment
    /// contract (alongside `objectiveai_agent_full_id` / `_remote` /
    /// `_instance_hierarchy`).
    pub objectiveai_agent_id: Option<String>,
    /// Agent's fully-qualified id (env `OBJECTIVEAI_AGENT_FULL_ID`).
    /// Currently unused — captured for parity with the objectiveai
    /// agent-environment contract.
    pub objectiveai_agent_full_id: Option<String>,
    /// Agent's remote ref (env `OBJECTIVEAI_AGENT_REMOTE`).
    /// Currently unused — captured for parity with the objectiveai
    /// agent-environment contract.
    pub objectiveai_agent_remote: Option<String>,
    /// Agent instance hierarchy (env `OBJECTIVEAI_AGENT_INSTANCE_HIERARCHY`).
    /// Defaults to `"mundus-animarum"` when the env var is unset. Identifies
    /// this agent instance; captured for parity with the objectiveai
    /// agent-environment contract.
    pub objectiveai_agent_instance_hierarchy: String,
}

impl Config {
    /// The state root (env `OBJECTIVEAI_STATE_DIR`). All state files
    /// live directly under it; assumed to already exist.
    pub fn state_dir(&self) -> PathBuf {
        self.state_dir.clone()
    }
}

/// Build the runtime config from the process environment.
pub fn load_config() -> Config {
    ConfigBuilder::init_from_env().unwrap_or_default().build()
}

// ---------------------------------------------------------------------------
// Command entry point
// ---------------------------------------------------------------------------

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

/// Parse `args` and dispatch to the matching command handler, returning the
/// command's JSON result.
///
/// Arguments are parsed **before** the context is built, so `--help` /
/// `--version` (and parse errors) don't require the environment to be
/// configured. The context — and thus config loading — is constructed only
/// once a real command is about to run.
pub async fn run<I, T>(args: I) -> Result<Value, Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;
    let ctx = Context::new();
    cli.command.handle(&ctx).await
}
