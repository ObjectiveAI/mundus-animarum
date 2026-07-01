//! `mcp mundus-animarum begin` — a thin launcher, not the server itself.
//!
//! Ensures the daemon is up (the host spawns our `daemon begin`, per the plugin
//! manifest's `daemon: true`), subscribe-reads the MCP server's connect URL from
//! the `"mcp"` lockfile, and returns it as the objectiveai
//! [`Mcp`](objectiveai_sdk::cli::command::plugins::run::Mcp) announcement —
//! which `main` serializes to the `{"type":"mcp","url":...}` line the host
//! parses. The server itself persists in the daemon.
//!
//! Takes no arguments — per-session identity flows in from the
//! `X-OBJECTIVEAI-*` request headers at connect time.

use clap::Args as ClapArgs;
use futures::StreamExt;
use objectiveai_sdk::cli::command::daemon::spawn as daemon_spawn;
use objectiveai_sdk::cli::command::plugin::PluginExecutor;
use objectiveai_sdk::cli::command::plugins::run::{Mcp, McpType};

use crate::context::Context;
use crate::error::Error;

#[derive(Debug, ClapArgs)]
pub struct Args {}

impl Args {
    pub(crate) async fn run(self, ctx: &Context) -> Result<serde_json::Value, Error> {
        // 1. Ensure the daemon is up. The SDK daemon launches our `daemon begin`
        //    (per the manifest's `daemon: true`), which runs the MCP server and
        //    publishes its URL to the `"mcp"` lockfile. A local executor keeps
        //    the plugin stdin/stdout capture confined to this command — the
        //    plain CLI commands must not construct one.
        let executor = PluginExecutor::new();
        let mut stream = daemon_spawn::execute(
            &executor,
            daemon_spawn::Request {
                path_type: daemon_spawn::Path::DaemonSpawn,
                dangerous_advanced: None,
                base: Default::default(),
            },
            None,
        )
        .await
        .map_err(|e| Error::Other(format!("daemon spawn: {e}")))?;
        if let Some(item) = stream.next().await {
            item.map_err(|e| Error::Other(format!("daemon spawn: {e}")))?;
        }

        // 2. Wait for the daemon's MCP server to publish its connect URL.
        let lock_dir = ctx.config.state_dir().join("locks");
        let url = objectiveai_sdk::lockfile::wait_read(&lock_dir, "mcp")
            .await
            .map_err(|e| Error::Other(format!("mcp lockfile: {e}")))?;

        // 3. Announce it; `main` prints this as the `{"type":"mcp","url":...}`
        //    line the host parses.
        serde_json::to_value(Mcp {
            r#type: McpType::Mcp,
            url,
        })
        .map_err(|e| Error::Other(format!("serialize mcp announcement: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The announcement must serialize to exactly the objectiveai plugin
    /// MCP-URL shape the host parses: `{"type":"mcp","url":...}`.
    #[test]
    fn announcement_is_objectiveai_mcp_shape() {
        let announcement = Mcp {
            r#type: McpType::Mcp,
            url: "http://127.0.0.1:54321".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&announcement).unwrap(),
            r#"{"type":"mcp","url":"http://127.0.0.1:54321"}"#,
        );
    }
}
