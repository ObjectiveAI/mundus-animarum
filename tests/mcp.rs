//! Integration tests for the mundus-animarum **MCP tools**, driven by
//! objectiveai **mock agents** whose tool calls are scripted with the `calls`
//! field. The host brings up the plugin's MCP server for the completion; the
//! plugin reads the caller's identity from the two `X-OBJECTIVEAI-AGENT-*`
//! headers (soul owner = agent full id, subscription owner = agent instance
//! hierarchy). Tool results are read back out of the completion stream, and
//! effects are verified cross-channel through the identity-agnostic CLI —
//! never the database or filesystem.
//!
//! Every MCP tool is covered: `get`, `list`, `set`, `delete`,
//! `subscribe_key`, `subscribe_soul`, `unsubscribe_key`, `unsubscribe_soul`,
//! `notifications`.

mod common;

use common::{Plugin, call, mcp_agent, tool};
use serde_json::json;

/// `set` writes the agent's own soul and echoes the value; `get` reads it
/// back. The write is confirmed cross-channel via the CLI on the agent's
/// captured full id.
#[tokio::test]
async fn mcp_set_and_get() {
    let p = Plugin::new("mcp_set_and_get");
    let agent = mcp_agent(vec![
        call(tool("set", json!({ "key": "color", "value": "blue" }))),
        call(tool("get", json!({ "key": "color" }))),
    ]);
    let s = p.spawn(agent).await;
    s.assert_no_errors();
    s.assert_called("set");
    s.assert_called("get");

    // set echoed the value, get read it back (both "blue").
    assert_eq!(s.tool_jsons(), vec![json!("blue"), json!("blue")]);

    // The MCP set wrote the agent's OWN soul — read it via the CLI on the
    // agent's full id (the soul owner the plugin saw).
    p.cli_get(s.full_id(), "color").await.assert_result(json!("blue"));
}

/// `get` with an explicit `agent_full_id` reads another agent's soul; without
/// it, your own (here empty).
#[tokio::test]
async fn mcp_get_other_soul() {
    let p = Plugin::new("mcp_get_other_soul");
    // Pre-seed the other agent's soul via the CLI.
    p.cli_set("other-soul", "shared", "hello").await.assert_no_errors();

    let agent = mcp_agent(vec![
        // cross-soul read
        call(tool("get", json!({ "key": "shared", "agent_full_id": "other-soul" }))),
        // own soul (empty) → null
        call(tool("get", json!({ "key": "shared" }))),
    ]);
    let s = p.spawn(agent).await;
    s.assert_no_errors();
    assert_eq!(s.tool_jsons(), vec![json!("hello"), json!(null)]);
}

/// `list` returns a soul's keys (sorted) — your own by default, or another
/// agent's when `agent_full_id` is given, exactly like `get`'s targeting.
#[tokio::test]
async fn mcp_list() {
    let p = Plugin::new("mcp_list");
    // pre-seed another agent's soul via the CLI.
    p.cli_set("other-soul", "alpha", "1").await.assert_no_errors();
    p.cli_set("other-soul", "beta", "2").await.assert_no_errors();

    let agent = mcp_agent(vec![
        call(tool("list", json!({}))), // own soul, still empty → []
        call(tool("set", json!({ "key": "y", "value": "1" }))),
        call(tool("set", json!({ "key": "x", "value": "2" }))),
        call(tool("list", json!({}))), // own soul → [x, y] (sorted)
        call(tool("list", json!({ "agent_full_id": "other-soul" }))), // → [alpha, beta]
    ]);
    let s = p.spawn(agent).await;
    s.assert_no_errors();
    s.assert_called("list");

    let r = s.tool_jsons();
    // own soul: empty before the sets, sorted keys after.
    assert!(r.contains(&json!([])), "expected an empty own-soul listing in {r:?}");
    assert!(r.contains(&json!(["x", "y"])), "expected sorted own-soul keys in {r:?}");
    // another agent's soul, sorted.
    assert!(
        r.contains(&json!(["alpha", "beta"])),
        "expected the other soul's keys in {r:?}",
    );
}

/// `delete` removes a key from the agent's own soul and reports whether it
/// existed.
#[tokio::test]
async fn mcp_delete() {
    let p = Plugin::new("mcp_delete");
    let agent = mcp_agent(vec![
        call(tool("set", json!({ "key": "k", "value": "v" }))),
        call(tool("delete", json!({ "key": "k" }))), // true (existed)
        call(tool("get", json!({ "key": "k" }))),    // null (gone)
        call(tool("delete", json!({ "key": "k" }))), // false (already gone)
    ]);
    let s = p.spawn(agent).await;
    s.assert_no_errors();
    assert_eq!(
        s.tool_jsons(),
        vec![json!("v"), json!(true), json!(null), json!(false)],
    );
    // Confirm the soul is empty cross-channel.
    p.cli_get(s.full_id(), "k").await.assert_result(json!(null));
}

/// `subscribe_key` / `subscribe_soul` register watches owned by the agent's
/// instance hierarchy; verified cross-channel via the CLI `subscriptions` on
/// the captured AIH.
#[tokio::test]
async fn mcp_subscribe() {
    let p = Plugin::new("mcp_subscribe");
    let agent = mcp_agent(vec![
        call(tool("subscribe_key", json!({ "agent_full_id": "target-X", "key": "status" }))),
        call(tool("subscribe_soul", json!({ "agent_full_id": "target-Y" }))),
    ]);
    let s = p.spawn(agent).await;
    s.assert_no_errors();
    s.assert_called("subscribe_key");
    s.assert_called("subscribe_soul");
    // both tools return null.
    assert_eq!(s.tool_jsons(), vec![json!(null), json!(null)]);

    // The subscriptions are owned by the agent's AIH — read them via the CLI.
    p.cli_subscriptions(s.aih()).await.assert_result(json!([
        { "target": "target-X", "key": "status" },
        { "target": "target-Y", "soul": true },
    ]));
}

/// `unsubscribe_key` / `unsubscribe_soul` remove the agent's watches; the CLI
/// `subscriptions` on the captured AIH confirms none remain.
#[tokio::test]
async fn mcp_unsubscribe() {
    let p = Plugin::new("mcp_unsubscribe");
    let agent = mcp_agent(vec![
        call(tool("subscribe_key", json!({ "agent_full_id": "target-X", "key": "status" }))),
        call(tool("subscribe_soul", json!({ "agent_full_id": "target-Y" }))),
        call(tool("unsubscribe_key", json!({ "agent_full_id": "target-X", "key": "status" }))),
        call(tool("unsubscribe_soul", json!({ "agent_full_id": "target-Y" }))),
    ]);
    let s = p.spawn(agent).await;
    s.assert_no_errors();
    p.cli_subscriptions(s.aih()).await.assert_result(json!([]));
}

/// `notifications` returns the agent's pending soul-change notifications and
/// resolves them. The deterministic flow: a detached first turn subscribes to
/// a victim soul (script `[subscribe_soul, notifications]`); `agents wait`
/// barriers until it's committed; a CLI `set` changes the victim soul; the
/// resumed turn re-runs the script and its `notifications` read drains the
/// soul change. Every step is sequenced — no timing hacks.
#[tokio::test]
async fn mcp_notifications_basic() {
    let p = Plugin::new("mcp_notifications_basic");
    let victim = "victim-agent";

    let agent = mcp_agent(vec![
        call(tool("subscribe_soul", json!({ "agent_full_id": victim }))),
        call(tool("notifications", json!({}))),
    ]);

    // Turn 1 (detached): subscribe commits; the notifications read sees
    // nothing (no change yet). Barrier until the instance lock is released.
    let aih = p.spawn_detached(agent).await;
    p.agents_wait(&aih).await;

    // Fire the soul change — the AIH is now a subscriber of the victim.
    p.cli_set(victim, "k", "v").await.assert_no_errors();

    // Turn 2 (resume, same AIH): re-subscribe (idempotent), then the
    // notifications read drains the soul change.
    let s = p.resume(&aih, "go").await;
    s.assert_no_errors();
    assert_eq!(
        s.notification_reads(),
        vec![json!({ "notifications": [{ "target": victim, "soul": true }], "remaining": 0 })],
    );
}

/// `notifications` honors `count` (never more than `count` per read) and
/// reports `remaining`. One CLI `set` of a new key fires BOTH the agent's key
/// subscription and its soul subscription on the victim; the resumed turn's
/// two `count:1` reads drain them one at a time — key first (key subs are
/// ordered before soul) with one remaining, then the soul with none.
#[tokio::test]
async fn mcp_notifications_count_and_remaining() {
    let p = Plugin::new("mcp_notifications_count");
    let victim = "victim-agent";

    let agent = mcp_agent(vec![
        call(tool("subscribe_key", json!({ "agent_full_id": victim, "key": "k1" }))),
        call(tool("subscribe_soul", json!({ "agent_full_id": victim }))),
        call(tool("notifications", json!({ "count": 1 }))),
        call(tool("notifications", json!({ "count": 1 }))),
    ]);

    let aih = p.spawn_detached(agent).await;
    p.agents_wait(&aih).await;

    // Setting the new key k1 fires the (victim,k1) key sub AND the victim soul sub.
    p.cli_set(victim, "k1", "v").await.assert_no_errors();

    let s = p.resume(&aih, "go").await;
    s.assert_no_errors();

    // count:1 → key first with one remaining, then the soul with none.
    assert_eq!(
        s.notification_reads(),
        vec![
            json!({ "notifications": [{ "target": victim, "key": "k1" }], "remaining": 1 }),
            json!({ "notifications": [{ "target": victim, "soul": true }], "remaining": 0 }),
        ],
    );
}

/// Reading the watched data resolves the notification. The agent watches the
/// victim's key `k` (script `[subscribe_key, get, notifications]`); after a
/// CLI `set` of that key fires the notification, the resumed turn's `get` of
/// the watched key reads "v" AND clears the pending notification, so the
/// following `notifications` read is empty. The get→notifications ordering is
/// fixed by the script, so the assertion is deterministic.
#[tokio::test]
async fn mcp_notifications_cleared_by_read() {
    let p = Plugin::new("mcp_notifications_cleared");
    let victim = "victim-agent";

    let agent = mcp_agent(vec![
        call(tool("subscribe_key", json!({ "agent_full_id": victim, "key": "k" }))),
        call(tool("get", json!({ "key": "k", "agent_full_id": victim }))),
        call(tool("notifications", json!({}))),
    ]);

    let aih = p.spawn_detached(agent).await;
    p.agents_wait(&aih).await;

    p.cli_set(victim, "k", "v").await.assert_no_errors();

    let s = p.resume(&aih, "go").await;
    s.assert_no_errors();

    // The get read the watched key (resolving the notification) …
    assert!(
        s.tool_jsons().contains(&json!("v")),
        "expected the get of the watched key to return 'v'; got {:?}",
        s.tool_jsons(),
    );
    // … so the notifications read is empty.
    assert_eq!(
        s.notification_reads(),
        vec![json!({ "notifications": [], "remaining": 0 })],
    );
}

/// The soul-scope counterpart of [`mcp_notifications_cleared_by_read`]:
/// listing the key set resolves a soul notification. The agent watches the
/// victim's SOUL (script `[subscribe_soul, list, notifications]`); after a CLI
/// `set` of a new key fires the soul notification, the resumed turn's `list`
/// of the watched soul reads [k] AND clears it, so the following
/// `notifications` read is empty.
#[tokio::test]
async fn mcp_notifications_cleared_by_list() {
    let p = Plugin::new("mcp_notifications_cleared_list");
    let victim = "victim-agent";

    let agent = mcp_agent(vec![
        call(tool("subscribe_soul", json!({ "agent_full_id": victim }))),
        call(tool("list", json!({ "agent_full_id": victim }))),
        call(tool("notifications", json!({}))),
    ]);

    let aih = p.spawn_detached(agent).await;
    p.agents_wait(&aih).await;

    p.cli_set(victim, "k", "v").await.assert_no_errors();

    let s = p.resume(&aih, "go").await;
    s.assert_no_errors();

    // The list read the watched soul (resolving the notification) and saw k …
    assert!(
        s.tool_jsons().contains(&json!(["k"])),
        "expected the list of the watched soul to return [k]; got {:?}",
        s.tool_jsons(),
    );
    // … so the notifications read is empty.
    assert_eq!(
        s.notification_reads(),
        vec![json!({ "notifications": [], "remaining": 0 })],
    );
}
