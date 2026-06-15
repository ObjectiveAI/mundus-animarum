//! Per-session identity, recorded from request headers by
//! [`super::header_session_manager::HeaderSessionManager`] and consumed by
//! every tool via [`super::MundusAnimarumMcp::resolve_session`].
//!
//! Two identities flow in per session, sourced on every connect:
//!
//!   - [`HEADER_ARGUMENTS`] (`X-OBJECTIVEAI-ARGUMENTS`) carries a JSON object
//!     of agent arguments. We read `agent_full_id` (and, as a fallback,
//!     `agent_instance_hierarchy`) case-insensitively.
//!   - [`HEADER_AGENT_INSTANCE_HIERARCHY`]
//!     (`X-OBJECTIVEAI-AGENT-INSTANCE-HIERARCHY`) is the session-global
//!     instance-hierarchy chain — the preferred source for the AIH.
//!
//! Both are optional here; a tool that needs one it wasn't given returns an
//! `invalid_params` error. The registry is in-memory only: objectiveai
//! re-sends these headers on every connect, so a process restart is
//! invisible — the lazy header capture in the session manager rebuilds the
//! entry from the next request.

use std::collections::HashMap;
use std::sync::Arc;

use rmcp::transport::common::server_side_http::SessionId;
use tokio::sync::RwLock;

/// HTTP header carrying a JSON object of agent arguments. We look up
/// `agent_full_id` / `agent_instance_hierarchy` case-insensitively.
pub const HEADER_ARGUMENTS: &str = "X-OBJECTIVEAI-ARGUMENTS";

/// HTTP header carrying the session-global agent instance hierarchy. The
/// preferred source for the AIH (the subscription owner).
pub const HEADER_AGENT_INSTANCE_HIERARCHY: &str = "X-OBJECTIVEAI-AGENT-INSTANCE-HIERARCHY";

/// The identities pulled from the request headers and pinned to the rmcp
/// session in memory.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// The caller's own agent full id — the soul owner/reader for the
    /// `get` / `set` / `delete` tools. `None` if no source carried it.
    pub agent_full_id: Option<String>,
    /// The caller's agent instance hierarchy — the owner of subscriptions
    /// and notifications. `None` if no source carried it.
    pub agent_instance_hierarchy: Option<String>,
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
