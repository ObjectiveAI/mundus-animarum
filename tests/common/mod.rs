//! Integration-test harness for mundus-animarum.
//!
//! Every test drives the prebuilt `objectiveai` host in the repo's
//! `.objectiveai/` (populated by `test.sh`) through the SDK
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
use objectiveai_sdk::cli::command::binary::{BinaryExecutor, Error as ExecError};
use objectiveai_sdk::cli::command::plugins::run as plugins_run;
use objectiveai_sdk::cli::command::{AgentArguments, CommandExecutor};
use serde_json::Value;

/// The plugin coordinate installed under `.objectiveai/bin/plugins/` by
/// `test.sh` (matches the repo-root `objectiveai.json`).
const OWNER: &str = "ObjectiveAI";
const NAME: &str = "mundus-animarum";
const VERSION: &str = "0.1.0";

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

/// What a test asks the harness to self-subscribe the spawned agent to (on
/// the first chunk, via the CLI): a single key, or the whole key set.
#[derive(Clone)]
enum SelfSub {
    Key(String),
    Soul,
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
            .execute::<_, plugins_run::ResponseItem>(request, None)
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
        let spec = InlineAgentBaseWithFallbacksOrRemoteCommitOptional::AgentBase(
            InlineAgentBaseWithFallbacks {
                inner: agent,
                fallbacks: None,
            },
        );
        let agent = AgentSelector::Ref {
            agent: AgentRef::Resolved(spec),
        };
        self.collect_spawn(agent, message, args, None, None).await
    }

    /// Spawn an inline agent and, the first time the streamed completion
    /// contains `trigger`, run a plugin CLI command (`cli_args`) — pausing
    /// the stream read while it runs. Used for the notification flow: the
    /// agent subscribes (the `trigger`), a CLI `set`/`delete` fires the
    /// change, then the agent (held behind warmup calls by stdout
    /// backpressure while the read is paused) reads its notification. The
    /// fired command's result is stored on [`SpawnResult::triggered`].
    pub async fn spawn_then_cli(
        &self,
        agent: InlineAgentBase,
        trigger: &str,
        cli_args: &[&str],
    ) -> SpawnResult {
        let spec = InlineAgentBaseWithFallbacksOrRemoteCommitOptional::AgentBase(
            InlineAgentBaseWithFallbacks {
                inner: agent,
                fallbacks: None,
            },
        );
        let agent = AgentSelector::Ref {
            agent: AgentRef::Resolved(spec),
        };
        let trig = (
            trigger.to_string(),
            cli_args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
        self.collect_spawn(agent, "", None, Some(trig), None).await
    }

    /// Resume an existing agent instance (by its AIH) with a new message.
    /// The stored agent definition is reused and the SAME instance
    /// hierarchy is presented to the plugin — so a subscription a prior
    /// turn created is readable here (the AIH is per-spawn, but resume
    /// pins it). The mock advances past calls already satisfied in earlier
    /// turns to the next scripted call.
    pub async fn resume(&self, aih: &str, message: &str) -> SpawnResult {
        let (parent, instance) = split_owner(aih);
        let agent = AgentSelector::Instance {
            parent_agent_instance_hierarchy: Some(parent),
            agent_instance: instance,
        };
        self.collect_spawn(agent, message, None, None, None).await
    }

    /// Spawn an inline agent and, on the first chunk (once the agent's
    /// identity is known), subscribe that agent to its OWN soul via the CLI —
    /// before the agent runs its own `set`. Lets the agent's own (fast,
    /// in-completion) `set` fire a notification it then reads/clears, with the
    /// ordering fixed by the script. Breaks the no-self-subscribe constraint
    /// without the agent needing to know its content-hash full id.
    /// `SelfSub::Key` watches one key; `SelfSub::Soul` watches the key set.
    pub async fn spawn_self_sub_key(&self, agent: InlineAgentBase, key: &str) -> SpawnResult {
        self.spawn_self_sub(agent, SelfSub::Key(key.to_string())).await
    }

    /// Like [`Self::spawn_self_sub_key`] but a whole-soul (key-set) self
    /// subscription (cleared by `list`).
    pub async fn spawn_self_sub_soul(&self, agent: InlineAgentBase) -> SpawnResult {
        self.spawn_self_sub(agent, SelfSub::Soul).await
    }

    async fn spawn_self_sub(&self, agent: InlineAgentBase, sub: SelfSub) -> SpawnResult {
        let spec = InlineAgentBaseWithFallbacksOrRemoteCommitOptional::AgentBase(
            InlineAgentBaseWithFallbacks {
                inner: agent,
                fallbacks: None,
            },
        );
        let agent = AgentSelector::Ref {
            agent: AgentRef::Resolved(spec),
        };
        self.collect_spawn(agent, "", None, None, Some(sub)).await
    }

    /// Core: stream a seeded `agents spawn` for `selector` and collect the
    /// completion into a [`SpawnResult`].
    ///
    /// Two optional mid-stream actions (each fired once, result stored on
    /// [`SpawnResult::triggered`]):
    /// - `cli_trigger = (needle, args)`: run the plugin CLI `args` the first
    ///   time a streamed item contains `needle`.
    /// - `self_sub = Some(SelfSub::…)`: subscribe the agent to its own soul
    ///   (`subscriber = AIH`, `target = full id`) once both are known (the
    ///   first chunk) — a single key or the whole key set.
    async fn collect_spawn(
        &self,
        selector: AgentSelector,
        message: &str,
        args: Option<AgentArguments>,
        mut cli_trigger: Option<(String, Vec<String>)>,
        mut self_sub: Option<SelfSub>,
    ) -> SpawnResult {
        let req = agents_spawn::Request {
            path_type: agents_spawn::Path::AgentsSpawn,
            message: RequestMessage::Simple(message.to_string()),
            agent: selector,
            dangerous_advanced: Some(agents_spawn::RequestDangerousAdvanced {
                stream: Some(true),
                seed: Some(42),
                skip_lock: None,
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

            let serialized = serde_json::to_value(&item).map(|v| v.to_string()).unwrap_or_default();

            // Capture identity from chunks.
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

            // Self-subscribe: once the agent's identity is known, subscribe it
            // to its own soul via the CLI (before the agent's own set).
            if let Some(sub) = &self_sub {
                if let (Some(aih), Some(fid)) =
                    (&result.agent_instance_hierarchy, &result.agent_full_id)
                {
                    let (aih, fid) = (aih.clone(), fid.clone());
                    let sub = sub.clone();
                    self_sub = None;
                    result.triggered = Some(match sub {
                        SelfSub::Key(key) => self.cli_subscribe_key(&aih, &fid, &key).await,
                        SelfSub::Soul => self.cli_subscribe_soul(&aih, &fid).await,
                    });
                }
            }

            // CLI trigger: run a plugin CLI command on the first matching item.
            if let Some((needle, cli_args)) = &cli_trigger {
                if serialized.contains(needle.as_str()) {
                    let cli_args = cli_args.clone();
                    cli_trigger = None;
                    let argv: Vec<&str> = cli_args.iter().map(String::as_str).collect();
                    result.triggered = Some(self.cli(&argv).await);
                }
            }

            result.items.push(item);
        }
        result
    }
}

/// `n` warmup `get` calls (distinct keys so the mock treats them as distinct
/// turns; no `agent_full_id` ⇒ reads the agent's own empty soul, clearing
/// nothing). Inserted between a subscribe and a notifications read: while
/// [`Plugin::spawn_then_cli`] pauses the read to run the CLI change, the
/// warmup chunks overflow the stdout pipe and the agent blocks on
/// backpressure — so it can't reach `notifications` until the change lands.
/// Kept compact so the inline-agent JSON stays under the OS argv limit.
pub fn warmups(n: usize) -> Vec<mock::Call> {
    (0..n)
        .map(|i| call(tool("get", serde_json::json!({ "key": i.to_string() }))))
        .collect()
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

/// The collected output of one mock-agent completion (`agents spawn`).
#[derive(Default)]
pub struct SpawnResult {
    /// The raw streamed response items (chunks + any id).
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
    /// The result of the CLI command fired mid-stream by
    /// [`Plugin::spawn_then_cli`], if any.
    pub triggered: Option<RunResult>,
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
    /// parse are skipped). For a script with no warmups this is exactly the
    /// per-call results in call order.
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

    /// The first tool result that parses as JSON and contains `key` at the
    /// top level — e.g. the `notifications` result (`{"notifications":…}`).
    pub fn tool_result_with(&self, key: &str) -> Value {
        self.tool_results()
            .into_iter()
            .find_map(|t| {
                let v: Value = serde_json::from_str(&t).ok()?;
                v.get(key).is_some().then_some(v)
            })
            .unwrap_or_else(|| {
                panic!("no tool result with `{key}` among {:?}", self.tool_results())
            })
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
