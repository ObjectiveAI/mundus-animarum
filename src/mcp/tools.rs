//! The soul tools: `get` / `list` / `set` / `delete`, `subscribe_key` /
//! `subscribe_soul`, `unsubscribe_key` / `unsubscribe_soul`, and
//! `notifications`.
//!
//! Identity is per-session, not per-call. The soul owner (for set/delete) is
//! the session's agent full id; the subscription owner (for the
//! subscribe/unsubscribe/notifications tools, and the reader that `get`/`list`
//! clear notifications for) is the session's agent instance hierarchy — both
//! pulled from request headers (see [`super::session`]). The *target* of a
//! subscription, and the soul `get`/`list` read, is a tool argument
//! (defaulting to your own soul).

use rmcp::model::{CallToolResult, Content};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde_json::Value;

use super::MundusAnimarumMcp;
use crate::db::Scope;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetRequest {
    #[schemars(description = "The soul key to read.")]
    pub key: String,
    #[schemars(
        description = "Full id of the agent whose soul to read. Defaults to your own soul when omitted."
    )]
    pub agent_full_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListRequest {
    #[schemars(
        description = "Full id of the agent whose soul keys to list. Defaults to your own soul when omitted."
    )]
    pub agent_full_id: Option<String>,
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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NotificationsRequest {
    #[schemars(description = "Maximum number of notifications to return (1–10). Defaults to 10.")]
    pub count: Option<u32>,
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
        // An MCP read resolves the reader's notification: the reader (whose
        // subscription this read clears) is your instance hierarchy —
        // subscriptions/notifications are owned by it, not the agent full id.
        // The *target* soul is yours by default (your full id), or another
        // agent's when `agent_full_id` is given.
        let reader = state.agent_instance_hierarchy.as_str();
        let target = req
            .agent_full_id
            .as_deref()
            .unwrap_or(state.agent_full_id.as_str());
        let value = self.db.get_key(Some(reader), target, &req.key).await.map_err(db_err)?;
        let body = serde_json::to_string(&value.map_or(Value::Null, Value::String))
            .map_err(json_err)?;
        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(
        name = "list",
        description = "List every key in a soul — your own by default, or another agent's."
    )]
    async fn list(
        &self,
        Parameters(req): Parameters<ListRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        // Like `get`, but for the whole key set: an MCP listing resolves the
        // reader's soul notification — reader is your instance hierarchy; the
        // target soul is yours by default, or another agent's when given.
        let reader = state.agent_instance_hierarchy.as_str();
        let target = req
            .agent_full_id
            .as_deref()
            .unwrap_or(state.agent_full_id.as_str());
        let keys = self.db.list_keys(Some(reader), target).await.map_err(db_err)?;
        let body = serde_json::to_string(&keys).map_err(json_err)?;
        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(name = "set", description = "Create or overwrite a key in your soul.")]
    async fn set(
        &self,
        Parameters(req): Parameters<SetRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let agent = state.agent_full_id.as_str();
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
        let agent = state.agent_full_id.as_str();
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
        let subscriber = state.agent_instance_hierarchy.as_str();
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
        let subscriber = state.agent_instance_hierarchy.as_str();
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
        let subscriber = state.agent_instance_hierarchy.as_str();
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
        let subscriber = state.agent_instance_hierarchy.as_str();
        self.db
            .unsubscribe_soul(subscriber, &req.agent_full_id)
            .await
            .map_err(db_err)?;
        Ok(ok_null())
    }

    #[tool(
        name = "notifications",
        description = "Read up to `count` (max 10) of your pending soul-change notifications. \
                       Returned notifications are marked resolved and won't appear again until \
                       the soul changes once more; `remaining` reports how many are still pending."
    )]
    async fn notifications(
        &self,
        Parameters(req): Parameters<NotificationsRequest>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let state = self.resolve_session(&ctx.extensions).await?;
        let subscriber = state.agent_instance_hierarchy.as_str();
        let limit = req.count.unwrap_or(10).min(10);
        // Atomically claim (resolve) up to `limit` notifications and report how
        // many remain — safe under concurrent reads (see Db::take_notifications).
        let (notifications, remaining) = self
            .db
            .take_notifications(subscriber, limit)
            .await
            .map_err(db_err)?;
        // One JSON object per notification: a single-key change carries `key`;
        // a whole-soul (key-set) change carries `soul: true`.
        let items: Vec<Value> = notifications
            .into_iter()
            .map(|n| match n.scope {
                Scope::Key(key) => serde_json::json!({ "target": n.target, "key": key }),
                Scope::Soul => serde_json::json!({ "target": n.target, "soul": true }),
            })
            .collect();
        let body = serde_json::to_string(&serde_json::json!({
            "notifications": items,
            "remaining": remaining,
        }))
        .map_err(json_err)?;
        Ok(CallToolResult::success(vec![Content::text(body)]))
    }
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
