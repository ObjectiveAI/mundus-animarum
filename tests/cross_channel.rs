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
