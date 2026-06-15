//! The soul tools: `get` / `set` / `delete`, `subscribe_key` /
//! `subscribe_soul`, `unsubscribe_key` / `unsubscribe_soul`, and
//! `notifications`.
//!
//! Identity is per-session, not per-call. The soul owner (for get/set/delete)
//! is the session's agent full id; the subscription owner (for the
//! subscribe/unsubscribe/notifications tools) is the session's agent instance
//! hierarchy — both pulled from request headers (see [`super::session`]). The
//! *target* of a subscription is a tool argument.

use rmcp::model::{CallToolResult, Content};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde_json::Value;

use super::MundusAnimarumMcp;
use super::session::SessionState;
use crate::db::Scope;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetRequest {
    #[schemars(description = "The soul key to read.")]
    pub key: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SetRequest {
    #[schemars(description = "The soul key to write.")]
    pub key: String,
    #[schemars(description = "The value to store.")]
    pub value: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DeleteRequest {
    #[schemars(description = "The soul key to delete.")]
    pub key: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SubscribeKeyRequest {
    #[schemars(description = "Full id of the target agent whose soul to watch.")]
    pub agent_full_id: String,
    #[schemars(description = "The soul key to watch for value changes / deletion.")]
    pub key: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SubscribeSoulRequest {
    #[schemars(description = "Full id of the target agent whose soul (key set) to watch.")]
    pub agent_full_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UnsubscribeKeyRequest {
    #[schemars(description = "Full id of the target agent to stop watching.")]
    pub agent_full_id: String,
    #[schemars(description = "The soul key to stop watching.")]
    pub key: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UnsubscribeSoulRequest {
    #[schemars(description = "Full id of the target agent whose soul (key set) to stop watching.")]
    pub agent_full_id: String,
}

#[tool_router(router = soul_tools, vis = "pub")]
impl MundusAnimarumMcp {
    #[tool(name = "get", description = "Read the value of a key in your soul.")]
    async fn get(
        &self,
        Parameters(req): Parameters<GetRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let agent = require_agent_full_id(&state)?;
        let value = self.db.get_key(agent, agent, &req.key).await.map_err(db_err)?;
        let body = serde_json::to_string(&value.map_or(Value::Null, Value::String))
            .map_err(json_err)?;
        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(name = "set", description = "Create or overwrite a key in your soul.")]
    async fn set(
        &self,
        Parameters(req): Parameters<SetRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let agent = require_agent_full_id(&state)?;
        self.db.set_key(agent, &req.key, &req.value).await.map_err(db_err)?;
        // Echo the stored value back, mirroring the CLI's `set`.
        let body = serde_json::to_string(&Value::String(req.value)).map_err(json_err)?;
        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(
        name = "delete",
        description = "Delete a key from your soul. Returns whether a key was removed."
    )]
    async fn delete(
        &self,
        Parameters(req): Parameters<DeleteRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let agent = require_agent_full_id(&state)?;
        let existed = self.db.delete_key(agent, &req.key).await.map_err(db_err)?;
        let body = serde_json::to_string(&Value::Bool(existed)).map_err(json_err)?;
        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(
        name = "subscribe_key",
        description = "Watch a single key in another agent's soul for value changes / deletion."
    )]
    async fn subscribe_key(
        &self,
        Parameters(req): Parameters<SubscribeKeyRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let subscriber = require_agent_instance_hierarchy(&state)?;
        self.db
            .subscribe_key(subscriber, &req.agent_full_id, &req.key)
            .await
            .map_err(db_err)?;
        Ok(ok_null())
    }

    #[tool(
        name = "subscribe_soul",
        description = "Watch the whole key set of another agent's soul for additions / removals."
    )]
    async fn subscribe_soul(
        &self,
        Parameters(req): Parameters<SubscribeSoulRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let subscriber = require_agent_instance_hierarchy(&state)?;
        self.db
            .subscribe_soul(subscriber, &req.agent_full_id)
            .await
            .map_err(db_err)?;
        Ok(ok_null())
    }

    #[tool(
        name = "unsubscribe_key",
        description = "Stop watching a single key in another agent's soul."
    )]
    async fn unsubscribe_key(
        &self,
        Parameters(req): Parameters<UnsubscribeKeyRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let subscriber = require_agent_instance_hierarchy(&state)?;
        self.db
            .unsubscribe_key(subscriber, &req.agent_full_id, &req.key)
            .await
            .map_err(db_err)?;
        Ok(ok_null())
    }

    #[tool(
        name = "unsubscribe_soul",
        description = "Stop watching the key set of another agent's soul."
    )]
    async fn unsubscribe_soul(
        &self,
        Parameters(req): Parameters<UnsubscribeSoulRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let subscriber = require_agent_instance_hierarchy(&state)?;
        self.db
            .unsubscribe_soul(subscriber, &req.agent_full_id)
            .await
            .map_err(db_err)?;
        Ok(ok_null())
    }

    #[tool(
        name = "notifications",
        description = "List your pending soul-change notifications."
    )]
    async fn notifications(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let subscriber = require_agent_instance_hierarchy(&state)?;
        let notifications = self.db.notifications(subscriber).await.map_err(db_err)?;
        // One JSON object per pending notification: a single-key change carries
        // `key`; a whole-soul (key-set) change carries `soul: true`.
        let items: Vec<Value> = notifications
            .into_iter()
            .map(|n| match n.scope {
                Scope::Key(key) => serde_json::json!({ "target": n.target, "key": key }),
                Scope::Soul => serde_json::json!({ "target": n.target, "soul": true }),
            })
            .collect();
        let body = serde_json::to_string(&Value::Array(items)).map_err(json_err)?;
        Ok(CallToolResult::success(vec![Content::text(body)]))
    }
}

/// The caller's own agent full id, or an `invalid_params` error.
fn require_agent_full_id(state: &SessionState) -> Result<&str, ErrorData> {
    state.agent_full_id.as_deref().ok_or_else(|| {
        ErrorData::invalid_params(
            "agent full ID is required for agents operating outside of objectiveai".to_string(),
            None,
        )
    })
}

/// The caller's agent instance hierarchy (subscription owner), or an
/// `invalid_params` error.
fn require_agent_instance_hierarchy(state: &SessionState) -> Result<&str, ErrorData> {
    state.agent_instance_hierarchy.as_deref().ok_or_else(|| {
        ErrorData::invalid_params("agent instance hierarchy is required".to_string(), None)
    })
}

/// The JSON `null` success result shared by the (un)subscribe tools.
fn ok_null() -> CallToolResult {
    CallToolResult::success(vec![Content::text("null")])
}

fn db_err(e: crate::db::Error) -> ErrorData {
    ErrorData::internal_error(format!("db: {e}"), None)
}

fn json_err(e: serde_json::Error) -> ErrorData {
    ErrorData::internal_error(format!("serialize: {e}"), None)
}
