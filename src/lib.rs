//! mundus-animarum — command-line interface for the world of ObjectiveAI
//! agent souls.

mod agent_ref;
mod command;
mod context;
mod db;
pub mod error;
mod subscription;

mod delete;
mod get;
mod list;
mod notifications;
mod set;
mod subscribe;
mod unsubscribe;

mod run;

pub use run::*;
