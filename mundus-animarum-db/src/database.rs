use std::future::Future;

/// Storage backend for agent souls.
///
/// A *soul* is a mutable, self-authored `key → value` store bound to an
/// ObjectiveAI agent's immutable, content-addressed ID (a 22-character base62
/// string).
///
/// This trait is **identity-agnostic**: it has no notion of a "current" or
/// "self" agent. Every operation names the agent(s) it acts on by explicit ID.
/// Deciding whether a call targets the caller's own soul or another agent's —
/// and supplying the caller's ID — is the responsibility of the layer above
/// (the MCP server), not this trait.
pub trait Database: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// List every soul key owned by `agent`.
    fn list_keys(
        &self,
        agent: &str,
    ) -> impl Future<Output = Result<Vec<String>, Self::Error>> + Send;

    /// Retrieve the value of `agent`'s soul `key`, or `None` if unset.
    fn get_key(
        &self,
        agent: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send;

    /// Create or overwrite `agent`'s soul `key` with `value`.
    fn set_key(
        &self,
        agent: &str,
        key: &str,
        value: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Delete `agent`'s soul `key`. Returns `true` if a key was removed,
    /// `false` if it did not exist.
    fn delete_key(
        &self,
        agent: &str,
        key: &str,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
}
