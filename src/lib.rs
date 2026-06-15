//! mundus-animarum — command-line interface for the world of ObjectiveAI
//! agent souls.

mod command;
mod context;
mod db;
pub mod error;
pub mod mcp;

mod run;

pub use run::*;
