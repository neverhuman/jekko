//! F5 trust pipeline — the line between a toy and trusted autonomy.
//!
//! Every effectful call goes through a fail-closed [`authorize_tool_call`] chain
//! that returns a [`ToolLease`] or a [`ToolDenied`] (no lease → no call). Every
//! call (including denials) produces a bounded, secret-free [`ToolReceipt`].
//! Host actions are armed only through [`decide_arm`] — the single gate that
//! enforces structural taint (law #6). All trust/safety decisions live in Rust.
//!
//! This module composes the existing fail-closed primitives rather than
//! reinventing them: `zyal_core::Capability`/`TaintSet`/`ActionClass`,
//! `zyal_core::{contains_any_credential, contains_any_forbidden}`,
//! `crate::command_floor`, and `crate::hashing::sha256_hex`. It is the mechanism
//! the tool kernel executor (M9) and the watcher control surface wire into.

mod adapters;
mod auth;
mod registry;
mod run;

pub use adapters::{
    adapter_for, run_sealed, CodeExecAdapter, SandboxPolicy, TheoremProverAdapter,
    WorkflowCallAdapter,
};
pub use auth::{
    authorize_tool_call, combine_taint, decide_arm, ActionRequest, ArmDecision, AuthorizeRequest,
    DenyReason, ReceiptInput, ToolDenied, ToolLease, ToolReceipt,
};
pub use registry::{ToolAdapter, ToolDescriptor, ToolKind, ToolOutput, ToolRegistry};
pub use run::{run_tool, RunToolCtx, ToolCallOutcome, ToolMisuseGuard, ToolPhase};

#[cfg(test)]
pub(crate) use adapters::cap_output;

#[cfg(test)]
include!("tool_kernel/m9_tests.rs");
#[cfg(test)]
include!("tool_kernel/tests.rs");
