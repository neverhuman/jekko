use serde::{Deserialize, Serialize};
use serde_json::Value;
use zyal_core::{ActionClass, Capability, TaintSet};

use crate::command_floor;
use crate::hashing::sha256_hex;

/// Why an effectful call was refused. Stable, secret-free reason codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    SchemaInvalid,
    PermissionDenied,
    CapabilityDenied,
    CredentialPolicy,
    SandboxDenied,
    CommandFloor,
    UrlPolicy,
    BudgetExceeded,
    TaintViolation,
    ForbiddenContent,
}

/// A refused call. Carries the reason code + a bounded, secret-free detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDenied {
    pub reason: DenyReason,
    pub detail: String,
}

/// A granted permission to make one effectful call. NEVER stores raw secrets
/// (`redacted` is always true) — only capability ids + scoping metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolLease {
    pub lease_id: String,
    pub tool_id: String,
    pub node_id: String,
    pub capabilities: Vec<Capability>,
    pub sandbox_profile: String,
    pub granted_offset: u64,
    pub redacted: bool,
}

/// Everything the authorize chain needs to decide. The caller (M9) assembles it
/// from the tool descriptor + the node's incoming taint.
pub struct AuthorizeRequest<'a> {
    pub tool_id: &'a str,
    pub node_id: &'a str,
    pub capabilities: &'a [Capability],
    /// Whether the tool performs a host side effect (drives the sandbox guard).
    pub side_effecting: bool,
    /// The shell command, if this is a shell tool.
    pub command: Option<&'a str>,
    /// The target URL, if this is a fetch tool.
    pub url: Option<&'a str>,
    /// The tool input (scanned for credential/forbidden content).
    pub input: &'a Value,
    /// Taint of the data flowing into this call.
    pub incoming_taint: &'a TaintSet,
    /// The action class this call would arm (taint gate).
    pub action_class: ActionClass,
    /// The declared sandbox profile (required for side-effecting tools).
    pub sandbox_profile: &'a str,
    /// The run event offset at which the lease is granted.
    pub granted_offset: u64,
}

fn deny(reason: DenyReason, detail: impl Into<String>) -> ToolDenied {
    ToolDenied {
        reason,
        detail: detail.into(),
    }
}

/// Strip any `user:pass@` userinfo from a URL so it is safe to put in a
/// serialized, persisted error detail (basic-auth credentials must never leak).
fn redact_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let after = &url[scheme_end + 3..];
        if let Some(at) = after.find('@') {
            return format!("{}://<redacted>@{}", &url[..scheme_end], &after[at + 1..]);
        }
    }
    url.to_string()
}

/// The fail-closed authorize chain. Each guard runs in order; the FIRST failure
/// denies the call. Only a fully-clean request is granted a lease.
pub fn authorize_tool_call(req: &AuthorizeRequest) -> Result<ToolLease, ToolDenied> {
    // 1. schema — the input must be present.
    if req.input.is_null() {
        return Err(deny(DenyReason::SchemaInvalid, "tool input is null"));
    }
    // 2. capability rule — deny-always capabilities are never grantable.
    for cap in req.capabilities {
        if cap.is_deny_always() {
            return Err(deny(
                DenyReason::CapabilityDenied,
                format!("`{}` is never grantable", cap.id()),
            ));
        }
    }
    // 3. credential policy — the input must not carry a raw credential.
    let input_str = req.input.to_string();
    if let Some(pattern) = zyal_core::contains_any_credential(&input_str) {
        return Err(deny(
            DenyReason::CredentialPolicy,
            format!("input carries a credential marker `{pattern}`"),
        ));
    }
    // 4. sandbox — a side-effecting tool must declare a sandbox profile.
    if req.side_effecting && req.sandbox_profile.trim().is_empty() {
        return Err(deny(
            DenyReason::SandboxDenied,
            "side-effecting tool needs a sandbox profile",
        ));
    }
    // 4b. a `sealed` sandbox provides NO network (M9-cont: this makes the
    //     no-network guarantee REAL, not just documented) — `network.fetch` is
    //     therefore not grantable to it. Network tools must use a non-sealed
    //     profile that can actually provide isolated, policy-checked egress.
    if req.sandbox_profile.trim() == "sealed"
        && req.capabilities.contains(&Capability::NetworkFetch)
    {
        return Err(deny(
            DenyReason::CapabilityDenied,
            "`network.fetch` is not grantable to a `sealed` sandbox (no egress)",
        ));
    }
    // 5. command floor — never run a catastrophic command.
    if let Some(cmd) = req.command {
        if let Some(reason) = command_floor::blocked_reason(cmd) {
            return Err(deny(
                DenyReason::CommandFloor,
                format!("command blocked: {reason}"),
            ));
        }
    }
    // 6. url policy — scan for embedded credentials (marker, never the raw
    //    value), then deny local/file schemes. The detail is ALWAYS redacted so
    //    a `user:pass@host` url can never leak into a persisted receipt/log.
    if let Some(url) = req.url {
        if let Some(pattern) = zyal_core::contains_any_credential(url) {
            return Err(deny(
                DenyReason::CredentialPolicy,
                format!("url carries a credential marker `{pattern}`"),
            ));
        }
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("file:")
            || lower.contains("localhost")
            || lower.contains("127.0.0.1")
            || lower.contains("169.254.")
        {
            return Err(deny(
                DenyReason::UrlPolicy,
                format!("url not allowed: {}", redact_url(url)),
            ));
        }
    }
    // 7. taint — incoming taint must be safe to arm this action class (law #6).
    if !req.incoming_taint.can_arm(req.action_class) {
        return Err(deny(
            DenyReason::TaintViolation,
            "tainted data cannot arm this action without a declared sanitizer",
        ));
    }
    // 8. forbidden scan — belt-and-suspenders over the full input.
    if let Some(pattern) = zyal_core::contains_any_forbidden(&input_str) {
        return Err(deny(
            DenyReason::ForbiddenContent,
            format!("input matches forbidden pattern `{pattern}`"),
        ));
    }
    Ok(ToolLease {
        lease_id: format!("lease-{}-{}", req.node_id, req.granted_offset),
        tool_id: req.tool_id.to_string(),
        node_id: req.node_id.to_string(),
        capabilities: req.capabilities.to_vec(),
        sandbox_profile: req.sandbox_profile.to_string(),
        granted_offset: req.granted_offset,
        redacted: true,
    })
}

/// A bounded, secret-free receipt for one effectful call (including denials).
/// Inputs/outputs are stored only as hashes; raw bytes never persist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolReceipt {
    pub receipt_id: String,
    pub tool_id: String,
    pub node_id: String,
    pub phase: String,
    pub latency_ms: u64,
    pub cost_usd: f64,
    pub input_hash: String,
    pub output_hash: String,
    pub taint_in: Vec<String>,
    pub taint_out: Vec<String>,
    pub lease_id: Option<String>,
    pub deny_reason: Option<DenyReason>,
}

fn taint_labels(taint: &TaintSet) -> Vec<String> {
    taint
        .labels
        .iter()
        .filter_map(|label| serde_json::to_value(label).ok())
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

/// Parameters for building a [`ToolReceipt`]. Kept as a struct so the call site
/// reads clearly and new fields don't churn the signature.
pub struct ReceiptInput<'a> {
    pub receipt_id: &'a str,
    pub tool_id: &'a str,
    pub node_id: &'a str,
    pub phase: &'a str,
    pub latency_ms: u64,
    pub cost_usd: f64,
    pub input: &'a str,
    pub output: &'a str,
    pub taint_in: &'a TaintSet,
    pub taint_out: &'a TaintSet,
    pub lease_id: Option<String>,
    pub deny_reason: Option<DenyReason>,
}

impl ToolReceipt {
    /// Build a receipt, hashing input/output. FAILS (defence in depth) if a raw
    /// credential slipped into either — a receipt must never persist a secret.
    pub fn finalize(args: ReceiptInput) -> Result<Self, String> {
        if let Some(pattern) = zyal_core::contains_any_credential(args.input) {
            return Err(format!("receipt input carries credential `{pattern}`"));
        }
        if let Some(pattern) = zyal_core::contains_any_credential(args.output) {
            return Err(format!("receipt output carries credential `{pattern}`"));
        }
        Ok(Self {
            receipt_id: args.receipt_id.to_string(),
            tool_id: args.tool_id.to_string(),
            node_id: args.node_id.to_string(),
            phase: args.phase.to_string(),
            latency_ms: args.latency_ms,
            cost_usd: args.cost_usd,
            input_hash: sha256_hex(args.input.as_bytes()),
            output_hash: sha256_hex(args.output.as_bytes()),
            taint_in: taint_labels(args.taint_in),
            taint_out: taint_labels(args.taint_out),
            lease_id: args.lease_id,
            deny_reason: args.deny_reason,
        })
    }
}

/// A host action awaiting the arming gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    pub node_id: String,
    pub action_class: ActionClass,
    pub taint: TaintSet,
    pub description: String,
}

/// The arming decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum ArmDecision {
    Armed,
    Blocked { reason: String },
}

/// The ONLY path that arms a host action. Fail-closed on taint (law #6): tainted
/// data cannot arm an action class it is unsafe for, no matter what asked.
pub fn decide_arm(req: &ActionRequest) -> ArmDecision {
    if req.taint.can_arm(req.action_class) {
        ArmDecision::Armed
    } else {
        ArmDecision::Blocked {
            reason: format!(
                "taint {:?} cannot arm {:?} at `{}`",
                taint_labels(&req.taint),
                req.action_class,
                req.node_id
            ),
        }
    }
}

/// Combine the taint of several inputs flowing into one node (union).
pub fn combine_taint(inputs: &[TaintSet]) -> TaintSet {
    inputs
        .iter()
        .fold(TaintSet::default(), |acc, t| acc.join(t))
}
