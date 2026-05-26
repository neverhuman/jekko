//! zyal-supervisor — durable substrate for ambitious ZYAL SuperWorkflows.
//!
//! This crate intentionally starts small: it validates a [`SuperWorkflow`]
//! manifest, computes phase readiness from the dependency DAG, and persists
//! run / phase / task / memory / evidence / sign-off state in SQLite.
//!
//! Host runtimes (e.g. `jekko-runtime`, `sandboxctl`, `jankurai-runner`) can
//! wire this in without making the ZYAL compiler itself a long-running
//! process. The API is intentionally synchronous for Phase F4; an async
//! wrapper is a follow-up.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

pub mod model;
pub mod planner;
pub mod store;

pub use model::{
    ControllerPolicy, Gate, GateKind, GraphStore, MemoryPolicy, NetworkPolicy, ParityPolicy, Phase,
    PhaseSignoffMode, PhaseStatus, RepoGraphPolicy, SandboxMode, SandboxPolicy, SuperWorkflow,
    Task, TaskStatus, WriteScope,
};
pub use planner::{execution_layers, ready_phases, validate_manifest, ValidationError};
pub use store::{SupervisorStore, SCHEMA};
