//! Shared args + resolution for `subscribe` / `unsubscribe`.

use clap::{ArgGroup, Args};

use crate::context::Context;
use crate::db::Scope;

/// Exactly one of `--key` (a single key) or `--soul` (the whole key set),
/// plus the required `--agent-full-id` of the target agent to watch. The
/// subscription owner defaults to the configured instance hierarchy, narrowed
/// by the optional `--agent-instance` / `--parent-agent-instance-hierarchy`
/// selector.
#[derive(Debug, Args)]
#[command(group = ArgGroup::new("scope")
    .required(true)
    .multiple(false)
    .args(["key", "soul"]))]
pub struct SubscriptionArgs {
    /// Watch a single key's value changes / deletion.
    #[arg(long, group = "scope", value_name = "KEY")]
    pub key: Option<String>,
    /// Watch the whole soul — its key set (key additions / removals).
    #[arg(long, group = "scope")]
    pub soul: bool,
    /// Full id of the target agent whose soul to watch. Required. May be the
    /// caller's own full id — several agents can share a full id, so an agent
    /// is allowed to watch its own.
    #[arg(long)]
    pub agent_full_id: String,
    /// Own the subscription as `<configured AIH>/<agent_instance>` (or
    /// `<parent>/<agent_instance>` when `--parent-agent-instance-hierarchy`
    /// is given). Omitted ⇒ the configured instance hierarchy itself.
    #[arg(long)]
    pub agent_instance: Option<String>,
    /// Explicit parent hierarchy for `--agent-instance`. Only valid alongside
    /// it.
    #[arg(long, requires = "agent_instance")]
    pub parent_agent_instance_hierarchy: Option<String>,
}

/// A resolved subscription: who is watching (`caller`, an agent instance
/// hierarchy), whom they watch (`target`, an agent full id), and which part
/// of the soul (`scope`).
pub struct Resolved {
    pub caller: String,
    pub target: String,
    pub scope: Scope,
}

impl SubscriptionArgs {
    /// Resolve the subscription. The owner (`caller`) is the configured
    /// instance hierarchy narrowed by the `--agent-instance` /
    /// `--parent-agent-instance-hierarchy` selector. Self-subscription
    /// (watching your own full id) is allowed.
    pub fn resolve(self, ctx: &Context) -> Resolved {
        let caller = ctx.agent_instance_hierarchy(
            self.agent_instance,
            self.parent_agent_instance_hierarchy,
        );
        let target = self.agent_full_id;
        // The `scope` ArgGroup guarantees exactly one of key/soul is set, so
        // `key.is_none()` means `--soul` was given.
        let scope = match self.key {
            Some(key) => Scope::Key(key),
            None => Scope::Soul,
        };
        Resolved { caller, target, scope }
    }
}
