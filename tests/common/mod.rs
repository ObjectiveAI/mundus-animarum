//! Integration-test harness for mundus-animarum.
//!
//! Every test drives the prebuilt `objectiveai` host in the repo's
//! `.objectiveai/` (populated by `build.sh`) through the SDK
//! [`BinaryExecutor`]. A test gets an isolated state by setting
//! `OBJECTIVEAI_STATE` to its own name; the host bootstraps a fresh
//! per-state postgres on first command. Setup, actions, AND assertions
//! all go through the executor — **no test reads the database or the
//! filesystem directly**.
//!
//! Two surfaces are exercised:
//!   - the plugin's **CLI** commands, via `plugins run` ([`Plugin::run`]
//!     + the typed `cli_*` methods). Identity is fully controlled with
//!     explicit flags (`--agent-full-id`, `--agent-instance`,
//!     `--parent-agent-instance-hierarchy`).
//!   - the plugin's **MCP** tools, via objectiveai **mock agents** whose
//!     tool calls are scripted with the `calls` field ([`Plugin::spawn`]
//!     + [`mcp_agent`]). The mock agent's identity (the two
//!     `X-OBJECTIVEAI-AGENT-*` headers the plugin reads) is minted by the
//!     host and surfaced on each completion chunk, so cross-channel
//!     assertions read it back from [`SpawnResult::aih`] /
//!     [`SpawnResult::full_id`].
//!
//! ## Deterministic notification flow
//!
//! Notifications need an external change to land *between* a subscribe and
//! a read. The harness sequences this with no sleeps or timing hacks, using
//! objectiveai's own primitives:
//!   1. [`Plugin::spawn_detached`] — a **non-streaming** `agents spawn`. The
//!      host re-execs the completion as a detached subprocess and returns
//!      immediately with the minted AIH.
//!   2. [`Plugin::agents_wait`] — block until that completion has fully
//!      finalized and released the instance lock (the deterministic barrier;
//!      see the spawn source: stream-false needs a wait, stream-true does
//!      not).
//!   3. a synchronous CLI `set` fires the change.
//!   4. [`Plugin::resume`] — a **streaming** `agents spawn` by the SAME AIH.
//!      It reuses the stored agent + presents the same instance hierarchy to
//!      the plugin, so the subscription the first turn created is the reader
//!      here. Streaming, so its tool results are collected inline and it
//!      needs no trailing wait.
#![allow(dead_code)]

use std::path::PathBuf;

use futures::StreamExt;
use objectiveai_sdk::agent::{
    ClientObjectiveaiMcp, ClientObjectiveaiMcpPluginEntry, ClientObjectiveaiMcpPluginMcpServer,
    InlineAgentBase, InlineAgentBaseWithFallbacks,
    InlineAgentBaseWithFallbacksOrRemoteCommitOptional, mock,
};
use objectiveai_sdk::cli::command::agents::message::RequestMessage;
use objectiveai_sdk::cli::command::agents::selector::{AgentRef, AgentSelector};
use objectiveai_sdk::cli::command::agents::spawn as agents_spawn;
use objectiveai_sdk::cli::command::agents::wait as agents_wait;
use objectiveai_sdk::cli::command::binary::{BinaryExecutor, Error as ExecError};
use objectiveai_sdk::cli::command::plugins::run as plugins_run;
use objectiveai_sdk::cli::command::{AgentArguments, CommandExecutor};
use serde_json::Value;

/// The plugin coordinate installed under `.objectiveai/bin/plugins/` by
/// `test.sh` (matches the repo-root `objectiveai.json`).
const OWNER: &str = "ObjectiveAI";
const NAME: &str = "mundus-animarum";
const VERSION: &str = "0.1.1";

/// The plugin manifest's declared MCP-server name (what the agent's
/// `client_objectiveai_mcp` brings up).
const MCP_SERVER: &str = "mundus-animarum";

/// The LLM-visible tool-name prefix: the MCP server's advertised
/// `serverInfo.name` (`mundus-animarum`, see `src/mcp/mod.rs`) with `_`/`.`
/// normalized to `-` (a no-op here). Tools are `mundus-animarum_<tool>`.
const TOOL_PREFIX: &str = "mundus-animarum";

/// Absolute path to the repo's `.objectiveai/` — the executor's
/// `OBJECTIVEAI_DIR`. The crate manifest dir IS the repo root, so no
/// `.parent()` hop is needed (unlike a workspace member).
fn objectiveai_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".objectiveai")
}

// ──────────────────────────── mock-agent builders ──────────────────────────

/// One scripted tool call to a mundus-animarum MCP tool. `tool` is the
/// bare tool name (`get`, `set`, …); `args` is the call's JSON arguments
/// (serialized to the JSON-string the mock wants).
pub fn tool(tool: &str, args: Value) -> mock::CallToolCall {
    mock::CallToolCall {
        name: format!("{TOOL_PREFIX}_{tool}"),
        arguments: args.to_string(),
    }
}

/// A single-tool-call turn (no content) — the common scripted step.
pub fn call(tc: mock::CallToolCall) -> mock::Call {
    mock::Call {
        tool_calls: vec![tc],
        content: String::new(),
    }
}

/// A content-only "done" turn (no tool calls) — closes a script so the
/// completion finishes cleanly after the last tool result returns.
pub fn done() -> mock::Call {
    mock::Call {
        tool_calls: Vec::new(),
        content: "done".to_string(),
    }
}

/// Build an inline **mock** agent that exposes the mundus-animarum MCP
/// server and runs the scripted `calls` deterministically. `executable:
/// false` means only the MCP server is brought up (not the plugin's own
/// command tools). A trailing [`done`] turn is appended so the completion
/// always terminates after the final tool result.
///
/// The mock restarts its script from `calls[0]` on every *separate*
/// completion, so a [`Plugin::spawn_detached`] of `[subscribe, read]`
/// followed by a [`Plugin::resume`] runs the WHOLE script twice: once
/// before the external change (the read sees nothing) and once after (the
/// read sees it). Assertions look only at the resumed completion.
pub fn mcp_agent(mut calls: Vec<mock::Call>) -> InlineAgentBase {
    calls.push(done());
    let mut base = mock::AgentBase::default();
    base.calls = Some(calls);
    base.client_objectiveai_mcp = Some(ClientObjectiveaiMcp {
        objectiveai: None,
        plugins: vec![ClientObjectiveaiMcpPluginEntry {
            owner: OWNER.to_string(),
            name: NAME.to_string(),
            version: VERSION.to_string(),
            executable: false,
            mcp_servers: Some(vec![ClientObjectiveaiMcpPluginMcpServer {
                name: MCP_SERVER.to_string(),
                // mundus-animarum ignores X-OBJECTIVEAI-ARGUMENTS; identity
                // comes from the two agent headers, so no per-server args.
                arguments: None,
            }]),
        }],
        tools: Vec::new(),
    });
    InlineAgentBase::Mock(base)
}

// ──────────────────────────────── the harness ──────────────────────────────

/// A handle that runs mundus-animarum against one isolated objectiveai
/// state, via the SDK [`BinaryExecutor`].
pub struct Plugin {
    executor: BinaryExecutor,
    state: String,
}

impl Plugin {
    /// Build a handle for a test. `state` (pass the test's own name)
    /// becomes `OBJECTIVEAI_STATE`, giving the test a fresh isolated state
    /// the host bootstraps on first command.
    pub fn new(state: &str) -> Self {
        let dir = objectiveai_dir();
        let executor = BinaryExecutor::new(Some(dir.clone()))
            .env("OBJECTIVEAI_DIR", dir.to_string_lossy().into_owned())
            .env("OBJECTIVEAI_STATE", state)
            // Tear the host child down when the response stream is dropped,
            // so a panicking assertion mid-stream doesn't leak the process.
            .kill_on_drop(true);
        Self {
            executor,
            state: state.to_string(),
        }
    }

    // ── CLI surface (plugins run) ───────────────────────────────────────

    /// Run an arbitrary plugin command (the plugin's own argv) and collect
    /// the response into a [`RunResult`].
    pub async fn run(&self, args: Vec<String>) -> RunResult {
        self.run_with(args, None).await
    }

    /// Run a CLI command **as a specific agent** — its instance hierarchy is
    /// forwarded to the plugin as `ctx.caller()` (so identity-sensitive
    /// behavior, like which subscription a read would resolve, uses `aih`).
    pub async fn cli_as(&self, aih: &str, args: &[&str]) -> RunResult {
        let agent_args = AgentArguments {
            agent_instance_hierarchy: Some(aih.to_string()),
            ..Default::default()
        };
        self.run_with(args.iter().map(|s| s.to_string()).collect(), Some(agent_args))
            .await
    }

    /// Core CLI dispatch, optionally with a per-call agent identity.
    async fn run_with(&self, args: Vec<String>, agent_args: Option<AgentArguments>) -> RunResult {
        let label = args.join(" ");
        let request = plugins_run::Request {
            path_type: plugins_run::Path::PluginsRun,
            owner: OWNER.to_string(),
            name: NAME.to_string(),
            version: VERSION.to_string(),
            args,
            base: Default::default(),
        };
        let mut stream = self
            .executor
            .execute::<_, plugins_run::ResponseItem>(request, agent_args.as_ref())
            .await
            .unwrap_or_else(|e| panic!("[{}] execute `{label}`: {e}", self.state));

        let mut result = RunResult::default();
        while let Some(item) = stream.next().await {
            match item {
                Ok(plugins_run::ResponseItem::Notification(value)) => result.outputs.push(value),
                Ok(plugins_run::ResponseItem::Mcp(mcp)) => result.mcps.push(mcp),
                // The executor's per-line decode matches `cli::Error` before
                // `ResponseItem`, so plugin error frames usually surface as
                // `Err(Cli)`; handle the typed `Error` shape too.
                Ok(plugins_run::ResponseItem::Error(e)) => result.errors.push(e),
                Err(ExecError::Cli(e)) => result.errors.push(e),
                Err(other) => panic!("[{}] `{label}` harness error: {other}", self.state),
            }
        }
        result
    }

    /// Convenience for `&[&str]` argv.
    pub async fn cli(&self, args: &[&str]) -> RunResult {
        self.run(args.iter().map(|s| s.to_string()).collect()).await
    }

    /// `set --key <key> --value <value> --agent-full-id <agent>`.
    pub async fn cli_set(&self, agent: &str, key: &str, value: &str) -> RunResult {
        self.cli(&[
            "set",
            "--agent-full-id",
            agent,
            "--key",
            key,
            "--value",
            value,
        ])
        .await
    }

    /// `get --key <key> --agent-full-id <agent>`.
    pub async fn cli_get(&self, agent: &str, key: &str) -> RunResult {
        self.cli(&["get", "--agent-full-id", agent, "--key", key])
            .await
    }

    /// `delete --key <key> --agent-full-id <agent>`.
    pub async fn cli_delete(&self, agent: &str, key: &str) -> RunResult {
        self.cli(&["delete", "--agent-full-id", agent, "--key", key])
            .await
    }

    /// `list --agent-full-id <agent>`.
    pub async fn cli_list(&self, agent: &str) -> RunResult {
        self.cli(&["list", "--agent-full-id", agent]).await
    }

    /// `subscribe --key <key> --agent-full-id <target>` owned by
    /// `subscriber` (an AIH `<parent>/<instance>`; must contain a `/`).
    pub async fn cli_subscribe_key(
        &self,
        subscriber: &str,
        target: &str,
        key: &str,
    ) -> RunResult {
        let (parent, instance) = split_owner(subscriber);
        self.cli(&[
            "subscribe",
            "--agent-full-id",
            target,
            "--key",
            key,
            "--agent-instance",
            &instance,
            "--parent-agent-instance-hierarchy",
            &parent,
        ])
        .await
    }

    /// `subscribe --soul --agent-full-id <target>` owned by `subscriber`.
    pub async fn cli_subscribe_soul(&self, subscriber: &str, target: &str) -> RunResult {
        let (parent, instance) = split_owner(subscriber);
        self.cli(&[
            "subscribe",
            "--agent-full-id",
            target,
            "--soul",
            "--agent-instance",
            &instance,
            "--parent-agent-instance-hierarchy",
            &parent,
        ])
        .await
    }

    /// `unsubscribe --key <key> --agent-full-id <target>` owned by `subscriber`.
    pub async fn cli_unsubscribe_key(
        &self,
        subscriber: &str,
        target: &str,
        key: &str,
    ) -> RunResult {
        let (parent, instance) = split_owner(subscriber);
        self.cli(&[
            "unsubscribe",
            "--agent-full-id",
            target,
            "--key",
            key,
            "--agent-instance",
            &instance,
            "--parent-agent-instance-hierarchy",
            &parent,
        ])
        .await
    }

    /// `unsubscribe --soul --agent-full-id <target>` owned by `subscriber`.
    pub async fn cli_unsubscribe_soul(&self, subscriber: &str, target: &str) -> RunResult {
        let (parent, instance) = split_owner(subscriber);
        self.cli(&[
            "unsubscribe",
            "--agent-full-id",
            target,
            "--soul",
            "--agent-instance",
            &instance,
            "--parent-agent-instance-hierarchy",
            &parent,
        ])
        .await
    }

    /// `subscriptions` for the entity `subscriber` (an AIH `<parent>/<instance>`).
    pub async fn cli_subscriptions(&self, subscriber: &str) -> RunResult {
        let (parent, instance) = split_owner(subscriber);
        self.cli(&[
            "subscriptions",
            "--agent-instance",
            &instance,
            "--parent-agent-instance-hierarchy",
            &parent,
        ])
        .await
    }

    // ── MCP surface (mock-agent completions) ────────────────────────────

    /// Spawn an inline agent through the host (`agents spawn`, streaming,
    /// seeded) with an empty initial message, and collect the completion.
    /// Streaming returns only after the completion has fully finalized.
    pub async fn spawn(&self, agent: InlineAgentBase) -> SpawnResult {
        self.spawn_with(agent, "", None).await
    }

    /// Like [`Self::spawn`] but with an explicit initial message and an
    /// optional per-call identity override ([`AgentArguments`]).
    pub async fn spawn_with(
        &self,
        agent: InlineAgentBase,
        message: &str,
        args: Option<AgentArguments>,
    ) -> SpawnResult {
        let agent = Self::resolved(agent);
        self.collect_spawn(agent, message, args).await
    }

    /// Spawn an inline agent **non-streaming**: the host re-execs the
    /// completion as a detached subprocess and returns immediately with the
    /// minted agent instance hierarchy (the first `ResponseItem::Id`). The
    /// completion runs on detached and still holds the instance lock — pair
    /// with [`Self::agents_wait`] to barrier until it has fully finalized
    /// (and released the lock) before a [`Self::resume`] re-acquires it.
    pub async fn spawn_detached(&self, agent: InlineAgentBase) -> String {
        let agent = Self::resolved(agent);
        let req = agents_spawn::Request {
            path_type: agents_spawn::Path::AgentsSpawn,
            message: RequestMessage::Simple("go".to_string()),
            agent,
            dangerous_advanced: Some(agents_spawn::RequestDangerousAdvanced {
                stream: Some(false),
                seed: Some(42),
            }),
            base: Default::default(),
        };
        let mut stream = self
            .executor
            .execute::<_, agents_spawn::ResponseItem>(req, None)
            .await
            .unwrap_or_else(|e| panic!("[{}] spawn_detached execute: {e}", self.state));
        while let Some(item) = stream.next().await {
            match item {
                Ok(agents_spawn::ResponseItem::Id(hier)) => return hier,
                Ok(_) => {}
                Err(ExecError::Cli(e)) => {
                    panic!("[{}] spawn_detached error: {e:?}", self.state)
                }
                Err(other) => panic!("[{}] spawn_detached harness error: {other}", self.state),
            }
        }
        panic!("[{}] spawn_detached: no Id in response", self.state)
    }

    /// Resume an existing agent instance (by its AIH) with a new message,
    /// streaming. The stored agent definition is reused and the SAME
    /// instance hierarchy is presented to the plugin — so a subscription a
    /// prior turn created is the reader here. The mock re-runs its script
    /// from the start, so a `[subscribe, read]` script's read now sees any
    /// change that landed since the first turn.
    pub async fn resume(&self, aih: &str, message: &str) -> SpawnResult {
        let (parent, instance) = split_owner(aih);
        let agent = AgentSelector::Instance {
            parent_agent_instance_hierarchy: Some(parent),
            agent_instance: instance,
        };
        self.collect_spawn(agent, message, None).await
    }

    /// Block until the agent instance `aih` has fully finalized and persisted
    /// (`agents wait` → the lockfile's `wait_released`). The deterministic
    /// barrier after a [`Self::spawn_detached`] — returns exactly when the
    /// detached completion is done and the instance lock is free, with no
    /// polling or sleeps.
    pub async fn agents_wait(&self, aih: &str) {
        let (parent, instance) = split_owner(aih);
        let req = agents_wait::Request {
            path_type: agents_wait::Path::AgentsWait,
            agent: AgentSelector::Instance {
                parent_agent_instance_hierarchy: Some(parent),
                agent_instance: instance,
            },
            base: Default::default(),
        };
        let mut stream = self
            .executor
            .execute::<_, agents_wait::Response>(req, None)
            .await
            .unwrap_or_else(|e| panic!("[{}] agents wait `{aih}`: {e}", self.state));
        while let Some(item) = stream.next().await {
            match item {
                Ok(_) => {}
                Err(ExecError::Cli(e)) => panic!("[{}] agents wait error: {e:?}", self.state),
                Err(other) => panic!("[{}] agents wait harness error: {other}", self.state),
            }
        }
    }

    /// Wrap an inline agent as a resolved `AgentSelector::Ref`.
    fn resolved(agent: InlineAgentBase) -> AgentSelector {
        let spec = InlineAgentBaseWithFallbacksOrRemoteCommitOptional::AgentBase(
            InlineAgentBaseWithFallbacks {
                inner: agent,
                fallbacks: None,
            },
        );
        AgentSelector::Ref {
            agent: AgentRef::Resolved(spec),
        }
    }

    /// Core: stream a seeded `agents spawn` for `selector` (a `Ref` for a
    /// fresh agent, an `Instance` for a resume) and collect the completion
    /// into a [`SpawnResult`]. Streaming (`stream = true`), so it returns
    /// only once the completion has fully finalized and persisted — no
    /// `agents wait` is needed after it.
    async fn collect_spawn(
        &self,
        selector: AgentSelector,
        message: &str,
        args: Option<AgentArguments>,
    ) -> SpawnResult {
        let req = agents_spawn::Request {
            path_type: agents_spawn::Path::AgentsSpawn,
            message: RequestMessage::Simple(message.to_string()),
            agent: selector,
            dangerous_advanced: Some(agents_spawn::RequestDangerousAdvanced {
                stream: Some(true),
                seed: Some(42),
            }),
            base: Default::default(),
        };
        let mut stream = self
            .executor
            .execute::<_, agents_spawn::ResponseItem>(req, args.as_ref())
            .await
            .unwrap_or_else(|e| panic!("[{}] spawn execute: {e}", self.state));

        let mut result = SpawnResult::default();
        while let Some(item) = stream.next().await {
            let item = match item {
                Ok(item) => item,
                Err(ExecError::Cli(e)) => {
                    result.errors.push(e);
                    continue;
                }
                Err(other) => panic!("[{}] spawn harness error: {other}", self.state),
            };

            // Capture identity from chunks (the first `ResponseItem` is an
            // `Id`; chunks carry the same hierarchy plus the full id).
            if let agents_spawn::ResponseItem::Chunk(chunk) = &item {
                if result.agent_instance_hierarchy.is_none()
                    && !chunk.agent_instance_hierarchy.is_empty()
                {
                    result.agent_instance_hierarchy = Some(chunk.agent_instance_hierarchy.clone());
                }
                if result.agent_full_id.is_none() && !chunk.agent_full_id.is_empty() {
                    result.agent_full_id = Some(chunk.agent_full_id.clone());
                }
                if let Some(err) = &chunk.error {
                    result.chunk_errors.push(format!("{err:?}"));
                }
            }

            result.items.push(item);
        }
        result
    }
}

/// Split an AIH `<parent>/<instance>` on its LAST `/` into the parent
/// hierarchy and the leaf instance, the inverse of how the plugin builds
/// `<parent>/<instance>`. Panics if there's no `/` (every AIH a test
/// controls or captures carries a lineage).
fn split_owner(aih: &str) -> (String, String) {
    let (parent, instance) = aih
        .rsplit_once('/')
        .unwrap_or_else(|| panic!("subscriber AIH `{aih}` must contain a '/'"));
    (parent.to_string(), instance.to_string())
}

// ──────────────────────────────── results ──────────────────────────────────

/// The collected output of one CLI (`plugins run`) command.
#[derive(Default)]
pub struct RunResult {
    /// JSON result lines the plugin emitted (a command's single result, or
    /// a `{"type":"help",…}` line for `--help`).
    pub outputs: Vec<Value>,
    /// Error frames surfaced by the plugin or host.
    pub errors: Vec<objectiveai_sdk::cli::Error>,
    /// MCP-URL announcements (`mcp … begin`).
    pub mcps: Vec<plugins_run::Mcp>,
}

impl RunResult {
    /// Assert the command produced no error frames; returns `self`.
    pub fn assert_no_errors(&self) -> &Self {
        assert!(
            self.errors.is_empty(),
            "expected no errors, got: {:?}",
            self.errors,
        );
        self
    }

    /// The command's single JSON result (asserts no errors first).
    pub fn result(&self) -> &Value {
        self.assert_no_errors();
        self.outputs
            .first()
            .unwrap_or_else(|| panic!("no output among {:?}", self.outputs))
    }

    /// Assert the result equals `expected` (asserts no errors first).
    pub fn assert_result(&self, expected: Value) -> &Self {
        assert_eq!(self.result(), &expected, "unexpected command result");
        self
    }

    /// At least one error frame whose message contains `needle`.
    pub fn has_error_containing(&self, needle: &str) -> bool {
        self.errors
            .iter()
            .any(|e| e.message.to_string().contains(needle))
    }
}

/// The collected output of one mock-agent completion (`agents spawn` /
/// resume).
#[derive(Default)]
pub struct SpawnResult {
    /// The raw streamed response items (the leading id + chunks).
    pub items: Vec<agents_spawn::ResponseItem>,
    /// Host/executor error frames.
    pub errors: Vec<objectiveai_sdk::cli::Error>,
    /// Per-chunk completion errors (`chunk.error`), stringified.
    pub chunk_errors: Vec<String>,
    /// The agent instance hierarchy the host minted for this completion —
    /// the subscription/notification owner the plugin's MCP sees in the
    /// `X-OBJECTIVEAI-AGENT-INSTANCE-HIERARCHY` header.
    pub agent_instance_hierarchy: Option<String>,
    /// The agent full id — the soul owner the plugin's MCP sees in the
    /// `X-OBJECTIVEAI-AGENT-FULL-ID` header. Stable for an agent definition.
    pub agent_full_id: Option<String>,
}

impl SpawnResult {
    /// Assert neither a host error frame nor a completion error occurred.
    pub fn assert_no_errors(&self) -> &Self {
        assert!(
            self.errors.is_empty(),
            "expected no host errors, got: {:?}",
            self.errors,
        );
        assert!(
            self.chunk_errors.is_empty(),
            "expected no completion errors, got: {:?}",
            self.chunk_errors,
        );
        self
    }

    /// Every streamed item serialized to JSON and joined — the surface the
    /// tool-name / tool-arguments / tool-result assertions match against.
    pub fn stream_json(&self) -> String {
        self.items
            .iter()
            .map(|i| serde_json::to_value(i).expect("spawn item serializes").to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Assert the completion stream contains `needle` (asserts no errors first).
    pub fn assert_contains(&self, needle: &str) -> &Self {
        self.assert_no_errors();
        let stream = self.stream_json();
        assert!(
            stream.contains(needle),
            "expected `{needle}` in completion stream:\n{stream}",
        );
        self
    }

    /// Assert the completion called the named MCP tool (proxy-prefixed).
    pub fn assert_called(&self, tool_name: &str) -> &Self {
        self.assert_contains(&format!("{TOOL_PREFIX}_{tool_name}"))
    }

    /// Every tool-result message body (the `role:"tool"` `content`
    /// strings), in stream order — each is what an MCP tool returned (e.g.
    /// a `get` value, a `delete` bool, the `notifications` JSON).
    pub fn tool_results(&self) -> Vec<String> {
        let mut out = Vec::new();
        for item in &self.items {
            let agents_spawn::ResponseItem::Chunk(chunk) = item else {
                continue;
            };
            for msg in &chunk.messages {
                let v = serde_json::to_value(msg).unwrap_or(Value::Null);
                if v.get("role").and_then(Value::as_str) == Some("tool") {
                    if let Some(content) = v.get("content").and_then(Value::as_str) {
                        out.push(content.to_string());
                    }
                }
            }
        }
        out
    }

    /// Every tool result parsed as JSON, in stream order (results that don't
    /// parse are skipped) — exactly the per-call results in call order.
    pub fn tool_jsons(&self) -> Vec<Value> {
        self.tool_results()
            .iter()
            .filter_map(|t| serde_json::from_str(t).ok())
            .collect()
    }

    /// Every `notifications` tool result (objects with a `notifications`
    /// key), in stream order — one per `notifications` call.
    pub fn notification_reads(&self) -> Vec<Value> {
        self.tool_results()
            .into_iter()
            .filter_map(|t| {
                let v: Value = serde_json::from_str(&t).ok()?;
                v.get("notifications").is_some().then_some(v)
            })
            .collect()
    }

    /// The minted agent instance hierarchy (panics if no chunk carried one).
    pub fn aih(&self) -> &str {
        self.agent_instance_hierarchy
            .as_deref()
            .unwrap_or_else(|| panic!("no agent_instance_hierarchy in spawn stream"))
    }

    /// The agent full id (panics if no chunk carried one).
    pub fn full_id(&self) -> &str {
        self.agent_full_id
            .as_deref()
            .unwrap_or_else(|| panic!("no agent_full_id in spawn stream"))
    }
}
