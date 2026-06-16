//! Integration tests for the mundus-animarum **CLI** commands, driven
//! through the objectiveai host's `plugins run`. Identity is supplied
//! explicitly via flags, so each command is exercised against a chosen soul
//! / subscriber without depending on the agent environment. Verification is
//! by reading back through the same CLI — never the database or filesystem.

mod common;

use common::Plugin;
use serde_json::json;

/// `set` echoes the stored value; `get` reads it back; a missing key is
/// `null`.
#[tokio::test]
async fn cli_set_and_get() {
    let p = Plugin::new("cli_set_and_get");
    let agent = "agent-A";

    // set returns the value it stored.
    p.cli_set(agent, "color", "blue").await.assert_result(json!("blue"));
    // get reads it back.
    p.cli_get(agent, "color").await.assert_result(json!("blue"));
    // overwriting updates the value.
    p.cli_set(agent, "color", "green").await.assert_result(json!("green"));
    p.cli_get(agent, "color").await.assert_result(json!("green"));
    // a key that was never set is null.
    p.cli_get(agent, "missing").await.assert_result(json!(null));
    // souls are per-agent: a different agent doesn't see this key.
    p.cli_get("agent-B", "color").await.assert_result(json!(null));
}

/// `delete` returns whether a key existed, and removes it.
#[tokio::test]
async fn cli_delete() {
    let p = Plugin::new("cli_delete");
    let agent = "agent-A";

    p.cli_set(agent, "k", "v").await.assert_result(json!("v"));
    // deleting an existing key reports true …
    p.cli_delete(agent, "k").await.assert_result(json!(true));
    // … and the key is gone.
    p.cli_get(agent, "k").await.assert_result(json!(null));
    // deleting again (now absent) reports false.
    p.cli_delete(agent, "k").await.assert_result(json!(false));
    // deleting a never-set key reports false.
    p.cli_delete(agent, "never").await.assert_result(json!(false));
}

/// `list` returns the soul's keys, sorted; empty souls list as `[]`.
#[tokio::test]
async fn cli_list() {
    let p = Plugin::new("cli_list");
    let agent = "agent-A";

    // an empty soul lists nothing.
    p.cli_list(agent).await.assert_result(json!([]));

    p.cli_set(agent, "banana", "1").await.assert_no_errors();
    p.cli_set(agent, "apple", "2").await.assert_no_errors();
    p.cli_set(agent, "cherry", "3").await.assert_no_errors();

    // keys come back sorted.
    p.cli_list(agent).await.assert_result(json!(["apple", "banana", "cherry"]));

    // delete shrinks the listing.
    p.cli_delete(agent, "banana").await.assert_result(json!(true));
    p.cli_list(agent).await.assert_result(json!(["apple", "cherry"]));

    // a different agent's soul is independent (empty).
    p.cli_list("agent-B").await.assert_result(json!([]));
}

/// `subscribe` registers key / soul watches owned by an entity; the
/// `subscriptions` command lists them (key subscriptions first, each sorted),
/// and re-subscribing is idempotent.
#[tokio::test]
async fn cli_subscribe_and_subscriptions() {
    let p = Plugin::new("cli_subscribe_and_subscriptions");
    let sub = "lineage/watcher-1";

    // nothing subscribed yet.
    p.cli_subscriptions(sub).await.assert_result(json!([]));

    // subscribe to a single key and to a whole soul, each returns null.
    p.cli_subscribe_key(sub, "target-X", "status").await.assert_result(json!(null));
    p.cli_subscribe_soul(sub, "target-Y").await.assert_result(json!(null));

    // both show up; key subscriptions come before soul ones.
    p.cli_subscriptions(sub).await.assert_result(json!([
        { "target": "target-X", "key": "status" },
        { "target": "target-Y", "soul": true },
    ]));

    // re-subscribing is idempotent (no duplicate).
    p.cli_subscribe_key(sub, "target-X", "status").await.assert_result(json!(null));
    p.cli_subscriptions(sub).await.assert_result(json!([
        { "target": "target-X", "key": "status" },
        { "target": "target-Y", "soul": true },
    ]));

    // a different entity owns no subscriptions.
    p.cli_subscriptions("lineage/watcher-2").await.assert_result(json!([]));
}

/// `unsubscribe` removes a key / soul watch; removing an absent one is a
/// no-op (no error).
#[tokio::test]
async fn cli_unsubscribe() {
    let p = Plugin::new("cli_unsubscribe");
    let sub = "lineage/watcher";

    p.cli_subscribe_key(sub, "target-X", "status").await.assert_no_errors();
    p.cli_subscribe_soul(sub, "target-Y").await.assert_no_errors();

    // drop the key subscription — only the soul one remains.
    p.cli_unsubscribe_key(sub, "target-X", "status").await.assert_result(json!(null));
    p.cli_subscriptions(sub).await.assert_result(json!([
        { "target": "target-Y", "soul": true },
    ]));

    // drop the soul subscription — none remain.
    p.cli_unsubscribe_soul(sub, "target-Y").await.assert_result(json!(null));
    p.cli_subscriptions(sub).await.assert_result(json!([]));

    // unsubscribing something not subscribed is a harmless no-op.
    p.cli_unsubscribe_key(sub, "target-X", "status").await.assert_result(json!(null));
    p.cli_unsubscribe_soul(sub, "nope").await.assert_result(json!(null));
}
