use std::future::Future;

/// A remark left by one agent on another agent's soul key.
#[derive(Debug, Clone)]
pub struct Remark {
    /// Content-addressed ID of the agent that authored the remark.
    pub author: String,
    /// The remark text.
    pub body: String,
    /// Unix epoch seconds at which the remark was created.
    pub created: u64,
    /// Whether this remark had already been read before this fetch.
    pub read: bool,
}

/// Storage backend for agent souls and the remarks left on them.
///
/// A *soul* is a mutable, self-authored `key → value` store bound to an
/// ObjectiveAI agent's immutable, content-addressed ID (a 22-character base62
/// string). Other agents leave *remarks* on a soul's keys; the owning agent
/// reads them, and each remark tracks a read/unread state from the owner's
/// perspective.
///
/// This trait is **identity-agnostic**: it has no notion of a "current" or
/// "self" agent. Every operation names the agent(s) it acts on by explicit ID.
/// Deciding whether a call targets the caller's own soul or another agent's —
/// and supplying the caller's ID — is the responsibility of the layer above
/// (the MCP server), not this trait.
pub trait Database: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    // ---- Soul keys ----

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

    // ---- Remarks ----

    /// Record a remark from `author` on `target`'s soul `key`. The remark
    /// starts unread.
    fn add_remark(
        &self,
        author: &str,
        target: &str,
        key: &str,
        body: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// List remarks on `target`'s soul `key`, oldest first, skipping `offset`
    /// and returning at most `count`. When `unread_only` is set, only unread
    /// remarks are considered.
    ///
    /// SIDE EFFECT: every remark returned by this call is marked read.
    ///
    /// With `unread_only = false` the matched set is stable across pages
    /// (re-marking an already-read remark is a no-op), so `offset` paging is
    /// well-defined. With `unread_only = true` the returned remarks leave the
    /// unread set, so the natural usage is repeated `offset = 0` "drain" calls
    /// rather than walking offsets.
    fn list_remarks(
        &self,
        target: &str,
        key: &str,
        offset: u64,
        count: u32,
        unread_only: bool,
    ) -> impl Future<Output = Result<Vec<Remark>, Self::Error>> + Send;

    /// For `target`, the number of unread remarks per soul key: a
    /// `(key, unread_count)` pair for every key that has at least one unread
    /// remark. Does NOT mark anything read.
    fn unread_remarks(
        &self,
        target: &str,
    ) -> impl Future<Output = Result<Vec<(String, u64)>, Self::Error>> + Send;
}
