//! Server entrypoints. Call [`run`] (all-in-one), or split it via [`setup`]
//! + [`serve`] to own the `TcpListener` / wrap the `axum::Router` first.

use std::sync::Arc;

use objectiveai_sdk::cli::command::plugins::run::{Mcp, McpType};
use objectiveai_sdk::cli::plugins::Output;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio_util::sync::CancellationToken;

use super::MundusAnimarumMcp;
use super::header_session_manager::HeaderSessionManager;
use super::session::SessionRegistry;
use crate::db::Db;

pub async fn setup(
    address: &str,
    port: u16,
    db: Db,
) -> std::io::Result<(tokio::net::TcpListener, axum::Router)> {
    let registry = Arc::new(SessionRegistry::new());
    let server = MundusAnimarumMcp::new(registry.clone(), db);
    let session_manager = Arc::new(HeaderSessionManager::new(registry.clone(), server.clone()));
    let ct = CancellationToken::new();

    let service: StreamableHttpService<MundusAnimarumMcp, HeaderSessionManager> =
        StreamableHttpService::new(
            move || Ok(server.clone()),
            session_manager,
            StreamableHttpServerConfig {
                stateful_mode: true,
                sse_keep_alive: None,
                cancellation_token: ct.child_token(),
                ..Default::default()
            },
        );

    let router = axum::Router::new().fallback_service(service);
    let listener = tokio::net::TcpListener::bind(format!("{address}:{port}")).await?;
    Ok((listener, router))
}

pub async fn serve(listener: tokio::net::TcpListener, app: axum::Router) -> std::io::Result<()> {
    axum::serve(listener, app).await
}

/// All-in-one entrypoint: bind, announce, serve.
///
/// Once bound, emits one JSONL line on stdout — the typed
/// [`objectiveai_sdk::cli::plugins::Output::Mcp`] variant carrying the bound
/// URL (`{"type":"mcp","url":"http://127.0.0.1:PORT"}`) — so the objectiveai
/// host can dial it. Identities flow in per session via headers, not here.
pub async fn run(address: &str, port: u16, db: Db) -> std::io::Result<()> {
    let (listener, app) = setup(address, port, db).await?;
    let addr = listener.local_addr()?;
    let announcement = Output::Mcp(Mcp {
        r#type: McpType::Mcp,
        url: format!("http://{addr}"),
    });
    println!(
        "{}",
        serde_json::to_string(&announcement).expect("Output::Mcp serializes"),
    );
    serve(listener, app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The announcement must serialize to exactly the objectiveai plugin
    /// MCP-URL shape the host parses: `{"type":"mcp","url":...}`.
    #[test]
    fn announcement_is_objectiveai_mcp_shape() {
        let announcement = Output::Mcp(Mcp {
            r#type: McpType::Mcp,
            url: "http://127.0.0.1:54321".to_string(),
        });
        assert_eq!(
            serde_json::to_string(&announcement).unwrap(),
            r#"{"type":"mcp","url":"http://127.0.0.1:54321"}"#,
        );
    }
}
