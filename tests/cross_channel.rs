//! Cross-channel integration tests: the CLI and the MCP operate on the same
//! soul store for the same identity. A value written through one surface is
//! readable through the other.

mod common;

use common::{Plugin, call, mcp_agent, tool, warmups};
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
/// does. The agent subscribes (via MCP) to a victim key; on a marker, a CLI
/// `set` fires the notification and a CLI `get` of that key — run AS the
/// agent (so a regression that resolved-by-caller would clear it) — must
/// leave it pending; the agent's `notifications` read still sees it.
#[tokio::test]
async fn cli_get_does_not_resolve_notification() {
    let p = Plugin::new("xchan_cli_get_no_resolve");
    let victim = "victim-agent";

    let mut calls = vec![
        call(tool("subscribe_key", json!({ "agent_full_id": victim, "key": "k" }))),
        // marker: triggers the CLI commands once the subscribe has committed.
        call(tool("get", json!({ "key": "FIRED" }))),
    ];
    calls.extend(warmups(250));
    calls.push(call(tool("notifications", json!({}))));
    let agent = mcp_agent(calls);

    let s = p
        .spawn_then_clis(
            agent,
            "FIRED",
            &[
                &["set", "--agent-full-id", victim, "--key", "k", "--value", "v"],
                &["get", "--agent-full-id", victim, "--key", "k"],
            ],
        )
        .await;
    s.assert_no_errors();
    // the CLI get ran and read the value (sanity) — but must not resolve.
    assert_eq!(s.triggered.as_ref().expect("a CLI get ran").result(), &json!("v"));
    // the notification survived the CLI read; only the agent's MCP read resolves.
    assert_eq!(
        s.notification_reads(),
        vec![json!({ "notifications": [{ "target": victim, "key": "k" }], "remaining": 0 })],
    );
}

/// A CLI `list` does NOT resolve a soul notification — only an agent's own
/// MCP read does. The agent subscribes (via MCP) to a victim SOUL; a CLI
/// `set` of a new key fires the soul notification and a CLI `list` — run AS
/// the agent — must leave it pending.
#[tokio::test]
async fn cli_list_does_not_resolve_notification() {
    let p = Plugin::new("xchan_cli_list_no_resolve");
    let victim = "victim-agent";

    let mut calls = vec![
        call(tool("subscribe_soul", json!({ "agent_full_id": victim }))),
        call(tool("get", json!({ "key": "FIRED" }))),
    ];
    calls.extend(warmups(250));
    calls.push(call(tool("notifications", json!({}))));
    let agent = mcp_agent(calls);

    let s = p
        .spawn_then_clis(
            agent,
            "FIRED",
            &[
                &["set", "--agent-full-id", victim, "--key", "k", "--value", "v"],
                &["list", "--agent-full-id", victim],
            ],
        )
        .await;
    s.assert_no_errors();
    // the CLI list ran (sanity: it returned the new key) — but must not resolve.
    assert_eq!(s.triggered.as_ref().expect("a CLI list ran").result(), &json!(["k"]));
    // the soul notification survived the CLI read.
    assert_eq!(
        s.notification_reads(),
        vec![json!({ "notifications": [{ "target": victim, "soul": true }], "remaining": 0 })],
    );
}
