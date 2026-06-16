//! Per-session identity, recorded from request headers by
//! [`super::header_session_manager::HeaderSessionManager`] and consumed by
//! every tool via [`super::MundusAnimarumMcp::resolve_session`].
//!
//! Two identities flow in per session, each from its own required header
//! (re-sent on every connect):
//!
//!   - [`HEADER_AGENT_FULL_ID`] (`X-OBJECTIVEAI-AGENT-FULL-ID`) — the caller's
//!     own agent full id (the soul owner/reader for `get`/`set`/`delete`).
//!   - [`HEADER_AGENT_INSTANCE_HIERARCHY`]
//!     (`X-OBJECTIVEAI-AGENT-INSTANCE-HIERARCHY`) — the caller's instance
//!     hierarchy (the owner of subscriptions and notifications).
//!
//! Both are required: a connection (fresh or reconnect) is rejected if either
//! header is missing or empty (see [`super::header_session_manager`]). The
//! registry is in-memory only — the headers, re-sent on every connect, let a
//! process restart rebuild the entry transparently.

use std::collections::HashMap;
use std::sync::Arc;

use rmcp::transport::common::server_side_http::SessionId;
use tokio::sync::RwLock;

/// HTTP header carrying the caller's own agent full id.
pub const HEADER_AGENT_FULL_ID: &str = "X-OBJECTIVEAI-AGENT-FULL-ID";

/// HTTP header carrying the caller's agent instance hierarchy (the
/// subscription / notification owner).
pub const HEADER_AGENT_INSTANCE_HIERARCHY: &str = "X-OBJECTIVEAI-AGENT-INSTANCE-HIERARCHY";

/// The identities pulled from the request headers and pinned to the rmcp
/// session in memory. Both are always present — the session manager rejects
/// any connection missing either header.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// The caller's own agent full id — the soul owner/reader for the
    /// `get` / `set` / `delete` tools.
    pub agent_full_id: String,
    /// The caller's agent instance hierarchy — the owner of subscriptions
    /// and notifications.
    pub agent_instance_hierarchy: String,
}

/// In-memory map of `SessionId → SessionState`. Shared between the custom
/// session manager (records on initialize / lazy capture, drops on close)
/// and the tool handlers (look up on every call via the Mcp-Session-Id).
#[derive(Default, Debug, Clone)]
pub struct SessionRegistry {
    inner: Arc<RwLock<HashMap<SessionId, Arc<SessionState>>>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn record(&self, id: SessionId, state: Arc<SessionState>) {
        self.inner.write().await.insert(id, state);
    }

    pub async fn get(&self, id: &SessionId) -> Option<Arc<SessionState>> {
        self.inner.read().await.get(id).cloned()
    }

    pub async fn remove(&self, id: &SessionId) -> Option<Arc<SessionState>> {
        self.inner.write().await.remove(id)
    }
}
