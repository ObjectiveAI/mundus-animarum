//! Integration tests for the mundus-animarum **MCP tools**, driven by
//! objectiveai **mock agents** whose tool calls are scripted with the `calls`
//! field. The host brings up the plugin's MCP server for the completion; the
//! plugin reads the caller's identity from the two `X-OBJECTIVEAI-AGENT-*`
//! headers (soul owner = agent full id, subscription owner = agent instance
//! hierarchy). Tool results are read back out of the completion stream, and
//! effects are verified cross-channel through the identity-agnostic CLI —
//! never the database or filesystem.
//!
//! Every MCP tool is covered: `get`, `set`, `delete`, `subscribe_key`,
//! `subscribe_soul`, `unsubscribe_key`, `unsubscribe_soul`, `notifications`.

mod common;

use common::{Plugin, call, mcp_agent, tool, warmups};
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

/// Collect every notification object drained across all `notifications`
/// reads, in stream order. Robust to read/CLI-set timing: regardless of which
/// spread read first runs after the change, the union is the same.
fn drained(reads: &[serde_json::Value]) -> Vec<serde_json::Value> {
    reads
        .iter()
        .flat_map(|r| r["notifications"].as_array().cloned().unwrap_or_default())
        .collect()
}

/// `notifications` returns the agent's pending soul-change notifications and
/// resolves them. The agent subscribes to a victim soul; a CLI `set` (fired
/// when the subscribe is seen) changes it; the agent reads notifications
/// repeatedly across the completion, and exactly one read drains the soul
/// change (the rest are empty — resolved). Spreading the reads makes the test
/// robust to when the (process-spawn) `set` lands relative to the agent.
#[tokio::test]
async fn mcp_notifications_basic() {
    let p = Plugin::new("mcp_notifications_basic");
    let victim = "victim-agent";

    let mut calls = vec![call(tool("subscribe_soul", json!({ "agent_full_id": victim })))];
    for _ in 0..6 {
        calls.extend(warmups(40));
        calls.push(call(tool("notifications", json!({}))));
    }
    let agent = mcp_agent(calls);

    let s = p
        .spawn_then_cli(
            agent,
            "mundus-animarum_subscribe_soul",
            &["set", "--agent-full-id", victim, "--key", "k", "--value", "v"],
        )
        .await;
    s.assert_no_errors();
    assert!(s.triggered.as_ref().is_some_and(|t| t.errors.is_empty()));

    let reads = s.notification_reads();
    // The soul change is drained exactly once across the reads.
    assert_eq!(
        drained(&reads),
        vec![json!({ "target": victim, "soul": true })],
        "expected exactly the soul change drained once; reads = {reads:?}",
    );
    // Nothing left pending at the end.
    assert_eq!(reads.last().expect("a read")["remaining"], json!(0));
}

/// `notifications` honors `count` (never more than `count` per read) and
/// reports `remaining`. One CLI `set` of a new key fires BOTH the agent's key
/// subscription and its soul subscription on the victim; the spread `count:1`
/// reads drain them one at a time — key first (key subs are ordered before
/// soul) with one remaining, then the soul with none remaining.
#[tokio::test]
async fn mcp_notifications_count_and_remaining() {
    let p = Plugin::new("mcp_notifications_count");
    let victim = "victim-agent";

    let mut calls = vec![
        call(tool("subscribe_key", json!({ "agent_full_id": victim, "key": "k1" }))),
        call(tool("subscribe_soul", json!({ "agent_full_id": victim }))),
        // gate: fire the set once both subscribes have committed.
        call(tool("get", json!({ "key": "GATEKEY" }))),
    ];
    for _ in 0..6 {
        calls.extend(warmups(40));
        calls.push(call(tool("notifications", json!({ "count": 1 }))));
    }
    let agent = mcp_agent(calls);

    // setting the new key k1 fires the (victim,k1) key sub AND the victim soul sub.
    let s = p
        .spawn_then_cli(
            agent,
            "GATEKEY",
            &["set", "--agent-full-id", victim, "--key", "k1", "--value", "v"],
        )
        .await;
    s.assert_no_errors();

    let reads = s.notification_reads();
    // count:1 cap — no read ever returns more than one notification.
    for r in &reads {
        assert!(
            r["notifications"].as_array().expect("array").len() <= 1,
            "count:1 exceeded in {r:?}",
        );
    }
    // Both fired notifications drained, key before soul.
    assert_eq!(
        drained(&reads),
        vec![
            json!({ "target": victim, "key": "k1" }),
            json!({ "target": victim, "soul": true }),
        ],
        "expected key then soul drained; reads = {reads:?}",
    );
    // The read that drained the key reported one still pending (the soul);
    // the read that drained the soul reported none.
    let key_read = reads
        .iter()
        .find(|r| r["notifications"][0].get("key").is_some())
        .expect("a read that drained the key");
    assert_eq!(key_read["remaining"], json!(1));
    let soul_read = reads
        .iter()
        .find(|r| r["notifications"][0].get("soul").is_some())
        .expect("a read that drained the soul");
    assert_eq!(soul_read["remaining"], json!(0));
}

/// Reading the watched data resolves the notification: the agent is
/// self-subscribed to its own soul key (subscription created via the CLI on
/// the first chunk, before the agent acts), sets that key (firing the
/// notification), reads it with `get` (which clears the pending
/// notification), then `notifications` is empty. set→get→notifications run in
/// one completion, so their ordering — and these assertions — are
/// deterministic.
#[tokio::test]
async fn mcp_notifications_cleared_by_read() {
    let p = Plugin::new("mcp_notifications_cleared");

    // Warm up first so the CLI self-subscribe lands before the agent's set.
    let mut calls = warmups(250);
    calls.push(call(tool("set", json!({ "key": "k", "value": "v" })))); // fires the self key-sub
    calls.push(call(tool("get", json!({ "key": "k" })))); // reads "v" AND clears
    calls.push(call(tool("notifications", json!({})))); // empty (cleared)
    let agent = mcp_agent(calls);

    let s = p.spawn_self_sub_key(agent, "k").await;
    s.assert_no_errors();
    // the self-subscribe succeeded.
    assert!(s.triggered.as_ref().is_some_and(|t| t.errors.is_empty()));

    // the agent set its own soul and read the value back (deterministic).
    assert!(
        s.tool_jsons().contains(&json!("v")),
        "expected the agent's own set+get to round-trip 'v'",
    );
    // reading the watched key cleared the notification, so it is empty.
    let reads = s.notification_reads();
    assert_eq!(reads, vec![json!({ "notifications": [], "remaining": 0 })]);
}
