//! Custom `SessionManager`. Two non-default behaviors that together make
//! session-id-as-identity disappear (the spoof psyops uses):
//!
//! 1. **`has_session` always returns `Ok(true)`.** Tower never 401s; any
//!    session id the client presents is accepted for routing.
//! 2. **Lazy `(handle, worker)` mint on first POST.** When tower routes a
//!    request for an id the inner `LocalSessionManager` doesn't hold, we read
//!    the required `X-OBJECTIVEAI-AGENT-FULL-ID` /
//!    `X-OBJECTIVEAI-AGENT-INSTANCE-HIERARCHY` headers off the current
//!    message's injected `http::request::Parts` (rejecting the connection if
//!    either is missing), record `SessionState`, spawn the worker + service
//!    end, and drive the worker past its initial `InitializeRequest` wait
//!    state with a synthetic stub.
//!
//! Net effect: objectiveai re-sends the headers on every connect; state is
//! in-memory only; a process restart silently rebuilds the per-session entry
//! on the next request. No disk.

use std::sync::Arc;

use futures::Stream;
use rmcp::model::{
    ClientCapabilities, ClientJsonRpcMessage, ClientRequest, GetExtensions, Implementation,
    InitializeRequestParams, JsonRpcRequest, JsonRpcVersion2_0, NumberOrString, ProtocolVersion,
    Request, ServerJsonRpcMessage,
};
use rmcp::service::serve_server;
use rmcp::transport::TransportAdapterIdentity;
use rmcp::transport::WorkerTransport;
use rmcp::transport::streamable_http_server::session::SessionManager;
use rmcp::transport::streamable_http_server::session::local::{
    LocalSessionManager, LocalSessionManagerError, SessionConfig, SessionError,
    create_local_session,
};
use rmcp::transport::streamable_http_server::session::{ServerSseMessage, SessionId};

use super::MundusAnimarumMcp;
use super::session::{
    HEADER_AGENT_FULL_ID, HEADER_AGENT_INSTANCE_HIERARCHY, SessionRegistry, SessionState,
};

#[derive(Debug, Clone)]
pub struct HeaderSessionManager {
    inner: Arc<LocalSessionManager>,
    registry: Arc<SessionRegistry>,
    /// Used by `ensure_session` to spawn a service end onto each
    /// lazy-created worker.
    service: MundusAnimarumMcp,
}

impl HeaderSessionManager {
    pub fn new(registry: Arc<SessionRegistry>, service: MundusAnimarumMcp) -> Self {
        Self {
            inner: Arc::new(LocalSessionManager::default()),
            registry,
            service,
        }
    }

    /// Make sure the inner `LocalSessionManager` has a handle for `id`. If it
    /// already does, no-op. Otherwise capture the headers from the current
    /// message, record `SessionState`, mint a worker, attach a service, and
    /// feed a synthetic initialize so the worker is ready for the real client
    /// message in its main loop.
    async fn ensure_session(
        &self,
        id: &SessionId,
        message: &ClientJsonRpcMessage,
    ) -> Result<(), LocalSessionManagerError> {
        if self.inner.has_session(id).await? {
            return Ok(());
        }

        let state = extract_session_state(message).map_err(error_invalid_input)?;
        self.registry.record(id.clone(), Arc::new(state)).await;

        let (handle, worker) = create_local_session(id.clone(), SessionConfig::default());
        let transport = WorkerTransport::spawn(worker);

        // Service-side task. When the service ends (worker died, transport
        // closed) we drop the entry from both maps.
        let svc = self.service.clone();
        let id_for_close = id.clone();
        let registry_for_close = self.registry.clone();
        let inner_for_close = self.inner.clone();
        tokio::spawn(async move {
            let res = serve_server::<_, _, _, TransportAdapterIdentity>(svc, transport).await;
            if let Ok(svc) = res {
                let _ = svc.waiting().await;
            }
            let _ = registry_for_close.remove(&id_for_close).await;
            inner_for_close.sessions.write().await.remove(&id_for_close);
        });

        // Drive the worker past its initial `InitializeRequest` wait state.
        // The response is discarded; the real client's subsequent initialize
        // (if any) overwrites peer_info on the next pass.
        handle
            .initialize(synthetic_initialize_message())
            .await
            .map_err(|e| error_invalid_input(format!("synthetic initialize: {e}")))?;

        self.inner.sessions.write().await.insert(id.clone(), handle);
        Ok(())
    }
}

impl SessionManager for HeaderSessionManager {
    type Error = LocalSessionManagerError;
    type Transport = <LocalSessionManager as SessionManager>::Transport;

    async fn create_session(&self) -> Result<(SessionId, Self::Transport), Self::Error> {
        self.inner.create_session().await
    }

    async fn initialize_session(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<ServerJsonRpcMessage, Self::Error> {
        let state = extract_session_state(&message).map_err(error_invalid_input)?;
        self.registry.record(id.clone(), Arc::new(state)).await;
        self.inner.initialize_session(id, message).await
    }

    /// Always `Ok(true)`. Validity is established lazily by `ensure_session`
    /// reading headers off the very request that uses the id.
    async fn has_session(&self, _id: &SessionId) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn close_session(&self, id: &SessionId) -> Result<(), Self::Error> {
        let _ = self.registry.remove(id).await;
        self.inner.close_session(id).await
    }

    async fn create_stream(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        self.ensure_session(id, &message).await?;
        self.inner.create_stream(id, message).await
    }

    async fn accept_message(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<(), Self::Error> {
        self.ensure_session(id, &message).await?;
        self.inner.accept_message(id, message).await
    }

    async fn create_standalone_stream(
        &self,
        id: &SessionId,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        // GET path: no message, no headers to extract. The CLI's MCP client
        // uses POST.
        self.inner.create_standalone_stream(id).await
    }

    async fn resume(
        &self,
        id: &SessionId,
        last_event_id: String,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        self.inner.resume(id, last_event_id).await
    }
}

/// Minimal-but-valid `initialize` request used during lazy rehydration to
/// drive a freshly-spawned worker past its initial wait state.
pub fn synthetic_initialize_message() -> ClientJsonRpcMessage {
    let request = Request {
        method: Default::default(),
        params: InitializeRequestParams {
            meta: None,
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "mundus-animarum-restore-stub".into(),
                title: None,
                version: "0".into(),
                description: None,
                icons: None,
                website_url: None,
            },
        },
        extensions: Default::default(),
    };
    ClientJsonRpcMessage::Request(JsonRpcRequest {
        jsonrpc: JsonRpcVersion2_0,
        id: NumberOrString::Number(0),
        request: ClientRequest::InitializeRequest(request),
    })
}

/// Pull the per-session identities off the request HTTP parts. Both the
/// agent-full-id and agent-instance-hierarchy headers are required — a missing
/// HTTP parts extension or a missing/empty identity header is an error, which
/// rejects the connection (fresh or reconnect).
fn extract_session_state(message: &ClientJsonRpcMessage) -> Result<SessionState, String> {
    let parts = match message {
        ClientJsonRpcMessage::Request(req) => {
            req.request.extensions().get::<http::request::Parts>()
        }
        ClientJsonRpcMessage::Notification(not) => {
            not.notification.extensions().get::<http::request::Parts>()
        }
        _ => None,
    }
    .ok_or_else(|| "message missing injected HTTP parts extension".to_string())?;

    let agent_full_id = header_value(parts, HEADER_AGENT_FULL_ID)
        .ok_or_else(|| format!("missing or empty {HEADER_AGENT_FULL_ID} header"))?;
    let agent_instance_hierarchy = header_value(parts, HEADER_AGENT_INSTANCE_HIERARCHY)
        .ok_or_else(|| format!("missing or empty {HEADER_AGENT_INSTANCE_HIERARCHY} header"))?;

    Ok(SessionState {
        agent_full_id,
        agent_instance_hierarchy,
    })
}

/// A request header's trimmed value, or `None` if absent, non-UTF-8, or empty.
fn header_value(parts: &http::request::Parts, name: &str) -> Option<String> {
    parts
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn error_invalid_input(msg: String) -> LocalSessionManagerError {
    LocalSessionManagerError::SessionError(SessionError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        msg,
    )))
}
