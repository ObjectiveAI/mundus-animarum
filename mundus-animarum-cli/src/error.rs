//! CLI error type. Every command handler returns `Result<_, Error>`;
//! `run` (in `run.rs`) returns it to `main`, which renders the terminal
//! `Err` as an objectiveai SDK error frame.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Argument parsing failed, or clap wants to print `--help` /
    /// `--version` (those are informational, not real failures — `main`
    /// special-cases them via [`crate::run::is_informational`]).
    #[error("{0}")]
    Clap(#[from] clap::Error),
    /// A soul-store (postgres) operation failed.
    #[error("database error: {0}")]
    Db(#[from] mundus_animarum_db::Error),
    /// No agent full id was given and none is configured. Agents running
    /// inside objectiveai get `OBJECTIVEAI_AGENT_FULL_ID` from the
    /// environment; anything outside must pass `--agent-full-id`.
    #[error("agent full ID is required for agents operating outside of objectiveai")]
    AgentFullIdRequired,
    /// An agent tried to subscribe to (or unsubscribe from) its own soul.
    #[error("an agent cannot subscribe to its own soul")]
    SelfSubscription,
    /// Catch-all for everything without a more specific variant.
    #[error("{0}")]
    Other(String),
}
