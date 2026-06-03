use std::future::Future;

/// What a subscription / notification is scoped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// A single soul key — value changes and deletion of that key.
    Key(String),
    /// The target's whole key set — key additions and deletions.
    Soul,
}

/// A pending change notification for a subscriber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The agent whose soul changed.
    pub target: String,
    /// What changed.
    pub scope: Scope,
}

/// Storage backend for agent souls and soul-change subscriptions.
///
/// A *soul* is a mutable, self-authored `key → value` store bound to an
/// ObjectiveAI agent's immutable, content-addressed ID (a 22-character base62
/// string).
///
/// Agents can **subscribe** to another agent's soul — either to a single key
/// (value changes and deletion of that key) or to the whole key set (key
/// additions and deletions). Subscriptions form a coalesced notification queue:
/// at most one pending notification per subscription, cleared when the
/// subscriber reads the subscribed data (reading a key clears that key's
/// notification; listing the keys clears the soul notification).
///
/// This trait is **identity-agnostic**: it has no notion of a "current" or
/// "self" agent. Every operation names the agent(s) it acts on by explicit ID.
/// Deciding whether a call targets the caller's own soul or another agent's —
/// and supplying the caller's ID — is the responsibility of the layer above
/// (the MCP server), not this trait.
pub trait Database: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    // ---- Soul keys ----

    /// List every soul key owned by `target`.
    ///
    /// Performed by `reader`: clears `reader`'s soul subscription on `target`,
    /// if any (it marks the pending notification read).
    fn list_keys(
        &self,
        reader: &str,
        target: &str,
    ) -> impl Future<Output = Result<Vec<String>, Self::Error>> + Send;

    /// Retrieve the value of `target`'s soul `key`, or `None` if unset.
    ///
    /// Performed by `reader`: clears `reader`'s key subscription on
    /// `(target, key)`, if any (it marks the pending notification read).
    fn get_key(
        &self,
        reader: &str,
        target: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send;

    /// Create or overwrite `owner`'s soul `key` with `value`.
    ///
    /// Fires the key subscriptions on `(owner, key)`; if the key is new, also
    /// fires the soul subscriptions on `owner` (the key set grew).
    fn set_key(
        &self,
        owner: &str,
        key: &str,
        value: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Delete `owner`'s soul `key`. Returns `true` if a key was removed,
    /// `false` if it did not exist.
    ///
    /// If a key was removed, fires the key subscriptions on `(owner, key)` and
    /// the soul subscriptions on `owner` (the key set shrank).
    fn delete_key(
        &self,
        owner: &str,
        key: &str,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    // ---- Subscriptions ----

    /// Subscribe `subscriber` to value changes and deletion of `target`'s
    /// soul `key`. Idempotent; starts caught-up (no pending notification).
    fn subscribe_key(
        &self,
        subscriber: &str,
        target: &str,
        key: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Subscribe `subscriber` to additions and deletions in `target`'s key
    /// set. Idempotent; starts caught-up (no pending notification).
    fn subscribe_soul(
        &self,
        subscriber: &str,
        target: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Remove `subscriber`'s key subscription on `(target, key)`, if any.
    fn unsubscribe_key(
        &self,
        subscriber: &str,
        target: &str,
        key: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Remove `subscriber`'s soul subscription on `target`, if any.
    fn unsubscribe_soul(
        &self,
        subscriber: &str,
        target: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    // ---- Notification queue ----

    /// List `subscriber`'s pending (unread) notifications. Does NOT mark
    /// anything read — a notification is cleared only by reading the subscribed
    /// data (see [`get_key`](Self::get_key) / [`list_keys`](Self::list_keys)).
    fn notifications(
        &self,
        subscriber: &str,
    ) -> impl Future<Output = Result<Vec<Notification>, Self::Error>> + Send;
}
