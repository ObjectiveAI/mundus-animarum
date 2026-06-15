//! Shared args + resolution for `subscribe` / `unsubscribe`.

use clap::{ArgGroup, Args};
use mundus_animarum_db::Scope;

use crate::context::Context;
use crate::error::Error;

/// Exactly one of `--key` (a single key) or `--keys` (the whole key set),
/// plus the required `--agent-full-id` of the target agent to watch.
#[derive(Debug, Args)]
#[command(group = ArgGroup::new("scope")
    .required(true)
    .multiple(false)
    .args(["key", "keys"]))]
pub struct SubscriptionArgs {
    /// Watch a single key's value changes / deletion.
    #[arg(long, group = "scope", value_name = "KEY")]
    pub key: Option<String>,
    /// Watch the whole key set (key additions / removals).
    #[arg(long, group = "scope")]
    pub keys: bool,
    /// Full id of the target agent whose soul to watch. Required, and must
    /// not be the caller's own id.
    #[arg(long)]
    pub agent_full_id: String,
}

/// A resolved subscription: who is watching (`caller`), whom they watch
/// (`target`), and which part of the soul (`scope`).
pub struct Resolved {
    pub caller: String,
    pub target: String,
    pub scope: Scope,
}

impl SubscriptionArgs {
    /// Resolve against the caller identity, rejecting a self-subscription.
    pub fn resolve(self, ctx: &Context) -> Result<Resolved, Error> {
        let caller = ctx.caller()?.to_string();
        let target = self.agent_full_id;
        if target == caller {
            return Err(Error::SelfSubscription);
        }
        // The `scope` ArgGroup guarantees exactly one of key/keys is set, so
        // `key.is_none()` means `--keys` was given.
        let scope = match self.key {
            Some(key) => Scope::Key(key),
            None => Scope::Soul,
        };
        Ok(Resolved { caller, target, scope })
    }
}
