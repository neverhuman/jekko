//! `jekko-memory` — live cogcore wiring for Jekko session memory.
//!
//! [`MemoryService`] holds a [`cogcore::core::Core`] and bridges it to the live
//! session loop: ingest turns (`observe_turn`), serve recall for Jekko's own
//! prompt-context assembly (`recall_block`), and persist / restore the cogcore
//! WAL through an injected [`WalSink`] so memory survives restart.
//!
//! Boundary: jekko owns session + memory; it owns no runners. Jeryu owns
//! runner, sandbox, and edit execution.

#![forbid(unsafe_code)]

mod audit;
mod error;
mod service;
mod turn;
mod wal;

#[cfg(test)]
mod tests;

pub use audit::RecallAudit;
pub use error::MemoryError;
pub use service::{MemoryConfig, MemoryService};
pub use turn::TurnObservation;
pub use wal::{InMemoryWalSink, WalOpDto, WalRecord, WalSink};
