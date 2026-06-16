//! Integration-test harness for mundus-animarum.
//!
//! Each test drives the prebuilt `objectiveai` host in the repo's
//! `.objectiveai/` (populated by `test.sh`) through the SDK
//! [`BinaryExecutor`], running the installed mundus-animarum plugin via
//! `plugins run`. A test gets an isolated state by setting `OBJECTIVEAI_STATE`
//! to its own name; the host auto-bootstraps it on first command. Setup,
//! actions, and assertions all go through the executor — no test reads the
//! filesystem directly.
//!
//! Scaffolding only for now: a generic [`Plugin::run`] dispatcher. Typed
//! per-command helpers land alongside the feature tests that need them.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::OnceLock;

use futures::StreamExt;
use objectiveai_sdk::cli::command::binary::{BinaryExecutor, Error as ExecError};
use objectiveai_sdk::cli::command::plugins::run as plugins_run;
// The `execute` method lives on the `CommandExecutor` trait — it must be in
// scope to call it on `BinaryExecutor`.
use objectiveai_sdk::cli::command::CommandExecutor;
use serde_json::Value;

/// The repo's `.objectiveai/` — the executor's `OBJECTIVEAI_DIR`. The crate
/// manifest dir IS the repo root (Cargo.toml lives at the root), so no
/// `.parent()` hop is needed.
fn objectiveai_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".objectiveai")
}

/// The plugin coordinate `(owner, name, version)`, read once from the
/// repo-root `objectiveai.json`. Single source of truth: it matches what
/// `test.sh` installs and tracks `version.sh` bumps automatically.
fn coords() -> &'static (String, String, String) {
    static COORDS: OnceLock<(String, String, String)> = OnceLock::new();
    COORDS.get_or_init(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("objectiveai.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let manifest: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("parse objectiveai.json: {e}"));
        let field = |key: &str| {
            manifest
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("objectiveai.json missing string field `{key}`"))
                .to_string()
        };
        (field("owner"), field("name"), field("version"))
    })
}

/// A handle that runs mundus-animarum plugin commands against one isolated
/// objectiveai state, via the SDK [`BinaryExecutor`].
pub struct Plugin {
    executor: BinaryExecutor,
    state: String,
}

impl Plugin {
    /// Build a handle for a test. `state` (pass the test's own name) becomes
    /// `OBJECTIVEAI_STATE`, giving the test a fresh isolated state the host
    /// bootstraps on first command.
    pub fn new(state: &str) -> Self {
        let dir = objectiveai_dir();
        let executor = BinaryExecutor::new(Some(dir.clone()))
            .env("OBJECTIVEAI_DIR", dir.to_string_lossy().into_owned())
            .env("OBJECTIVEAI_STATE", state)
            // Tear the host child down when the response stream is dropped, so
            // a panicking assertion mid-stream doesn't leak the process.
            .kill_on_drop(true);
        Self {
            executor,
            state: state.to_string(),
        }
    }

    /// Run `plugins run <coords> -- <args>` (the plugin's argv) through the
    /// executor and collect the response stream. Panics on a harness/infra
    /// failure (spawn, IO, undecodable line) — those aren't under test.
    pub async fn run(&self, args: Vec<String>) -> RunResult {
        let label = args.join(" ");
        let (owner, name, version) = coords().clone();
        let request = plugins_run::Request {
            path_type: plugins_run::Path::PluginsRun,
            owner,
            name,
            version,
            args,
            base: Default::default(),
        };
        let mut stream = self
            .executor
            .execute::<_, plugins_run::ResponseItem>(request, None)
            .await
            .unwrap_or_else(|e| panic!("[{}] execute `{label}`: {e}", self.state));

        let mut result = RunResult::default();
        while let Some(item) = stream.next().await {
            match item {
                Ok(plugins_run::ResponseItem::Notification(value)) => result.outputs.push(value),
                Ok(plugins_run::ResponseItem::Mcp(mcp)) => result.mcps.push(mcp),
                // `BinaryExecutor` decodes `cli::Error`-shaped lines into the
                // `Err(Cli)` arm; handle the `ResponseItem::Error` shape too.
                Ok(plugins_run::ResponseItem::Error(e)) => result.errors.push(e),
                Err(ExecError::Cli(e)) => result.errors.push(e),
                Err(other) => panic!("[{}] `{label}` harness error: {other}", self.state),
            }
        }
        result
    }
}

/// The collected output of one plugin run.
#[derive(Default)]
pub struct RunResult {
    /// Terminal / notification JSON lines the plugin emitted (e.g. a `get`
    /// value, a help line).
    pub outputs: Vec<Value>,
    /// Error frames surfaced by the plugin or the host.
    pub errors: Vec<objectiveai_sdk::cli::Error>,
    /// MCP-URL announcements (`{"type":"mcp","url":…}`).
    pub mcps: Vec<plugins_run::Mcp>,
}

impl RunResult {
    /// Assert the run produced no error frames.
    pub fn assert_no_errors(&self) -> &Self {
        assert!(
            self.errors.is_empty(),
            "expected no errors, got: {:?}",
            self.errors,
        );
        self
    }
}
