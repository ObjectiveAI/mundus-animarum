//! Cross-channel integration tests: the CLI and the MCP operate on the same
//! soul store for the same identity. A value written through one surface is
//! readable through the other.

mod common;

use common::{Plugin, call, mcp_agent, tool};
use serde_json::json;

/// A value `set` via the CLI is readable via the MCP `get` tool (same soul).
#[tokio::test]
async fn cli_set_then_mcp_get() {
    let p = Plugin::new("xchan_cli_set_mcp_get");
    p.cli_set("shared-agent", "k", "from-cli").await.assert_no_errors();

    let agent = mcp_agent(vec![call(tool(
        "get",
        json!({ "key": "k", "agent_full_id": "shared-agent" }),
    ))]);
    let s = p.spawn(agent).await;
    s.assert_no_errors();
    assert_eq!(s.tool_jsons(), vec![json!("from-cli")]);
}

/// A value `set` via the MCP tool is readable via the CLI `get` / `list` on
/// the agent's own soul.
#[tokio::test]
async fn mcp_set_then_cli_get() {
    let p = Plugin::new("xchan_mcp_set_cli_get");
    let agent = mcp_agent(vec![call(tool(
        "set",
        json!({ "key": "k", "value": "from-mcp" }),
    ))]);
    let s = p.spawn(agent).await;
    s.assert_no_errors();

    let fid = s.full_id();
    p.cli_get(fid, "k").await.assert_result(json!("from-mcp"));
    p.cli_list(fid).await.assert_result(json!(["k"]));
}

/// A CLI `get` does NOT resolve notifications — only an agent's own MCP read
/// does. The agent watches a victim key (script `[subscribe_key,
/// notifications]`); after a CLI `set` fires the notification, a CLI `get` of
/// that key — run AS the agent, so a regression that resolved-by-caller would
/// clear it — must leave it pending; the resumed agent's own `notifications`
/// read still sees it.
#[tokio::test]
async fn cli_get_does_not_resolve_notification() {
    let p = Plugin::new("xchan_cli_get_no_resolve");
    let victim = "victim-agent";

    let agent = mcp_agent(vec![
        call(tool("subscribe_key", json!({ "agent_full_id": victim, "key": "k" }))),
        call(tool("notifications", json!({}))),
    ]);

    // Turn 1 (detached): subscribe to the victim's key k. Barrier.
    let aih = p.spawn_detached(agent).await;
    p.agents_wait(&aih).await;

    // Fire the change, then read the key via the CLI — run AS the agent. The
    // CLI read passes reader = None, so it must leave the notification pending.
    p.cli_set(victim, "k", "v").await.assert_no_errors();
    let read = p
        .cli_as(&aih, &["get", "--agent-full-id", victim, "--key", "k"])
        .await;
    read.assert_result(json!("v")); // sanity: the CLI get saw the value …

    // Turn 2 (resume): the agent's own MCP notifications read still sees it —
    // the CLI get did not resolve it.
    let s = p.resume(&aih, "go").await;
    s.assert_no_errors();
    assert_eq!(
        s.notification_reads(),
        vec![json!({ "notifications": [{ "target": victim, "key": "k" }], "remaining": 0 })],
    );
}

/// A CLI `list` does NOT resolve a soul notification — only an agent's own
/// MCP read does. The agent watches a victim SOUL (script `[subscribe_soul,
/// notifications]`); after a CLI `set` of a new key fires the soul
/// notification, a CLI `list` — run AS the agent — must leave it pending.
#[tokio::test]
async fn cli_list_does_not_resolve_notification() {
    let p = Plugin::new("xchan_cli_list_no_resolve");
    let victim = "victim-agent";

    let agent = mcp_agent(vec![
        call(tool("subscribe_soul", json!({ "agent_full_id": victim }))),
        call(tool("notifications", json!({}))),
    ]);

    let aih = p.spawn_detached(agent).await;
    p.agents_wait(&aih).await;

    // Fire a soul change (a new key), then list the soul via the CLI as the
    // agent. The CLI list must not resolve the soul notification.
    p.cli_set(victim, "k", "v").await.assert_no_errors();
    let read = p.cli_as(&aih, &["list", "--agent-full-id", victim]).await;
    read.assert_result(json!(["k"])); // sanity: the CLI list saw the new key …

    // Turn 2 (resume): the soul notification survived the CLI read.
    let s = p.resume(&aih, "go").await;
    s.assert_no_errors();
    assert_eq!(
        s.notification_reads(),
        vec![json!({ "notifications": [{ "target": victim, "soul": true }], "remaining": 0 })],
    );
}
