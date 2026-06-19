//! Advanced reasoning state machine for generic ZYAL port runs.
//!
//! This module was extracted from a single 919-LOC file. The public surface
//! is preserved via re-exports below so callers continue to use
//! `crate::reasoning_runner::*`.

mod orchestrator;
mod phases;
mod types;

#[cfg(test)]
mod tests;

pub use orchestrator::run_advanced_reasoning_tick_with_db;
pub use types::{AdvancedReasoningSummary, AdvancedReasoningTickReport};
