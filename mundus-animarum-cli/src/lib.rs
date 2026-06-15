//! mundus-animarum CLI library — command-line interface for the world of
//! ObjectiveAI agent souls.

pub mod commands;
pub mod context;
pub mod error;

mod run;

pub use run::*;
