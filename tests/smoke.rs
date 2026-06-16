//! Harness smoke test: proves objectiveai + the plugin are installed and the
//! host can run the plugin. Uses `--help`, which the plugin parses before
//! building its `Context`, so it needs no database — it exercises the plumbing
//! without depending on the postgres env contract.

mod common;

#[tokio::test]
async fn harness_smoke() {
    let plugin = common::Plugin::new("harness_smoke");
    let result = plugin.run(vec!["--help".into()]).await;
    result.assert_no_errors();
}
