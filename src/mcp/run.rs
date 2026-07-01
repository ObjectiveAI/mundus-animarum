//! Server entrypoints. Call [`run`] (all-in-one), or split it via [`setup`]
//! + [`serve`] to own the `TcpListener` / wrap the `axum::Router` first.

use std::path::Path;
use std::sync::Arc;

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

/// The daemon body: bind loopback, publish the connect URL, serve until death.
///
/// Binds an OS-assigned port on `127.0.0.1`, then publishes the resulting
/// `http://<ip>:<port>` into `<state_dir>/locks` under key `"mcp"` via
/// [`objectiveai_sdk::lockfile::try_acquire`] — which also enforces
/// single-instance: if another live daemon already holds the lock for this
/// state, we error rather than bind a competing server. The launcher
/// (`mcp mundus-animarum begin`) subscribe-reads this URL and announces it to
/// the host; this function produces no stdout — the lockfile is the only side
/// channel. Identities flow in per session via headers, not here.
pub async fn run(state_dir: &Path, db: Db) -> std::io::Result<()> {
    let (listener, app) = setup("127.0.0.1", 0, db).await?;

    // Publish the connect URL for the daemon: key "mcp", value
    // "http://<ip>:<port>", mapping an unspecified bind to loopback. The
    // `LockClaim` is held until process death (it leaks on drop by design);
    // we only check for a conflicting live holder.
    let addr = listener.local_addr()?;
    let connect_ip = match addr.ip() {
        std::net::IpAddr::V4(v4) if v4.is_unspecified() => {
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        }
        std::net::IpAddr::V6(v6) if v6.is_unspecified() => {
            std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
        }
        ip => ip,
    };
    let connect_url = format!("http://{}", std::net::SocketAddr::new(connect_ip, addr.port()));
    let lock_dir = state_dir.join("locks");
    if objectiveai_sdk::lockfile::try_acquire(&lock_dir, "mcp", &connect_url)
        .await
        .is_none()
    {
        return Err(std::io::Error::other(
            "another mundus-animarum instance already holds the mcp lock for this state",
        ));
    }

    serve(listener, app).await
}
