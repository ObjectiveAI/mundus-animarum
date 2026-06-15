//! mundus-animarum MCP server — a streamable-HTTP RMCP server exposing the
//! soul store as tools.
//!
//! No `main`, no `Config`: the only entrypoint is [`run`] (split into
//! [`setup`] + [`serve`] for callers that want to own the listener). It
//! reuses the crate's shared [`Db`](crate::db::Db).
//!
//! `agent_full_id` and the agent instance hierarchy are NOT parameters —
//! they land per-session from the `X-OBJECTIVEAI-ARGUMENTS` /
//! `X-OBJECTIVEAI-AGENT-INSTANCE-HIERARCHY` headers (see [`session`]),
//! recorded by [`header_session_manager`] and looked up by every tool via
//! [`MundusAnimarumMcp::resolve_session`].

mod header_session_manager;
mod run;
pub mod session;
mod tools;

pub use run::{run, serve, setup};

use std::sync::Arc;

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::router::tool::ToolRouter,
    handler::server::tool::ToolCallContext,
    model::{
        CallToolRequestParams, CallToolResult, Implementation, ListToolsResult,
        PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    transport::common::http_header::HEADER_SESSION_ID,
};

use crate::db::Db;
use session::{SessionRegistry, SessionState};

#[derive(Clone)]
pub struct MundusAnimarumMcp {
    pub tool_router: ToolRouter<Self>,
    sessions: Arc<SessionRegistry>,
    db: Db,
}

impl std::fmt::Debug for MundusAnimarumMcp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MundusAnimarumMcp").finish_non_exhaustive()
    }
}

impl MundusAnimarumMcp {
    pub fn new(sessions: Arc<SessionRegistry>, db: Db) -> Self {
        Self {
            tool_router: Self::soul_tools(),
            sessions,
            db,
        }
    }

    /// Resolve `Mcp-Session-Id → SessionState` for the in-flight request.
    /// `invalid_params` if the request carried no session id or it's unknown.
    async fn resolve_session(
        &self,
        extensions: &rmcp::model::Extensions,
    ) -> Result<Arc<SessionState>, ErrorData> {
        let parts = extensions.get::<http::request::Parts>().ok_or_else(|| {
            ErrorData::internal_error("missing http request parts on rmcp request".to_string(), None)
        })?;
        let id = parts
            .headers
            .get(HEADER_SESSION_ID)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ErrorData::invalid_params(format!("missing {HEADER_SESSION_ID} header"), None)
            })?;
        self.sessions
            .get(&id.to_owned().into())
            .await
            .ok_or_else(|| ErrorData::invalid_params(format!("unknown session: {id}"), None))
    }
}

impl ServerHandler for MundusAnimarumMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "mundus-animarum".into(),
                title: None,
                version: "1.0.0".into(),
                description: None,
                icons: None,
                website_url: None,
            },
            instructions: None,
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tcc = ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_router.get(name).cloned()
    }
}
