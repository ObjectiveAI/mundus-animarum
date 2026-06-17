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
    LocalSessionHandle, LocalSessionManager, LocalSessionManagerError, SessionConfig, SessionError,
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

    /// Mint a fresh worker for `id`: capture the headers off `message`,
    /// record `SessionState`, spawn the worker + its service end, and return
    /// the handle. The worker is NOT yet driven past its initial
    /// `InitializeRequest` wait and is NOT yet inserted into the inner
    /// manager — the caller does both, driving the worker with either the
    /// REAL initialize (resume path in [`Self::create_stream`], whose response
    /// must reach the client) or a synthetic one ([`Self::ensure_session`]).
    async fn mint_worker(
        &self,
        id: &SessionId,
        message: &ClientJsonRpcMessage,
    ) -> Result<LocalSessionHandle, LocalSessionManagerError> {
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

        Ok(handle)
    }

    /// Ensure the inner `LocalSessionManager` holds a worker for `id`, lazily
    /// minting one for a NON-initialize first message (the lazy-reconnect
    /// case: a request lands on a session this fresh instance never saw). The
    /// worker is driven past its initial `InitializeRequest` wait with a
    /// SYNTHETIC initialize so the real (non-initialize) message rides through
    /// its main loop. Initialize messages never reach here —
    /// [`Self::create_stream`] intercepts them and drives the REAL initialize
    /// so its `InitializeResult` reaches the client. A no-op when the session
    /// already exists.
    async fn ensure_session(
        &self,
        id: &SessionId,
        message: &ClientJsonRpcMessage,
    ) -> Result<(), LocalSessionManagerError> {
        if self.inner.has_session(id).await? {
            return Ok(());
        }
        let handle = self.mint_worker(id, message).await?;
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
        // Resume-initialize to a session this (possibly fresh) instance
        // doesn't hold. rmcp routes an initialize-carrying-a-session-id
        // through `create_stream`, NOT `initialize_session`. But
        // `inner.create_stream` delivers the message via a
        // `SessionEvent::ClientMessage`, and a freshly-minted worker's initial
        // state only advances on a `SessionEvent::InitializeRequest` (sent by
        // `handle.initialize`). A pushed `ClientMessage` at that state is never
        // processed, so the SSE closes with no event and the client sees
        // "stream ended before a complete event". Mint the worker, drive the
        // REAL initialize through the handle, and return its `InitializeResult`
        // as a one-item stream.
        if is_initialize(&message) && !self.inner.has_session(id).await? {
            let handle = self.mint_worker(id, &message).await?;
            let response = handle
                .initialize(message)
                .await
                .map_err(|e| error_invalid_input(format!("resume initialize: {e}")))?;
            self.inner.sessions.write().await.insert(id.clone(), handle);
            let item = ServerSseMessage {
                event_id: None,
                message: Some(Arc::new(response)),
                retry: None,
            };
            let stream: std::pin::Pin<
                Box<dyn Stream<Item = ServerSseMessage> + Send + Sync + 'static>,
            > = Box::pin(futures::stream::iter(vec![item]));
            return Ok(stream);
        }
        self.ensure_session(id, &message).await?;
        let inner = self.inner.create_stream(id, message).await?;
        let stream: std::pin::Pin<
            Box<dyn Stream<Item = ServerSseMessage> + Send + Sync + 'static>,
        > = Box::pin(inner);
        Ok(stream)
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

/// True when the message is itself an `initialize` request. Used by
/// [`HeaderSessionManager::create_stream`] to drive the REAL initialize
/// through the worker handle (rather than letting the inner manager push it as
/// a `ClientMessage`, which a freshly-minted worker never processes).
fn is_initialize(m: &ClientJsonRpcMessage) -> bool {
    matches!(
        m,
        ClientJsonRpcMessage::Request(r)
            if matches!(r.request, ClientRequest::InitializeRequest(_))
    )
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
