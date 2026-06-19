use serde::{Deserialize, Serialize};
use serde_json::Value;
use zyal_core::{ActionClass, TaintSet};

use crate::run_store::SourceMode;

use super::{
    authorize_tool_call, AuthorizeRequest, DenyReason, ReceiptInput, ToolAdapter, ToolDenied,
    ToolDescriptor, ToolLease, ToolOutput, ToolReceipt,
};

/// Phases a tool call passes through (drives the `tool_call_update` frames).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPhase {
    Queued,
    Authorizing,
    Denied,
    Started,
    Progress,
    Retrying,
    Succeeded,
    Failed,
    Cached,
    Misused,
}

/// The full outcome of one tool call — the phase sequence (emitted as
/// `tool_call_update` frames by the caller), the receipt, and the lease/denial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallOutcome {
    pub phases: Vec<ToolPhase>,
    pub receipt: ToolReceipt,
    pub lease: Option<ToolLease>,
    pub denied: Option<ToolDenied>,
    pub output: Option<ToolOutput>,
}

/// Inputs to one `run_tool` invocation.
pub struct RunToolCtx<'a> {
    pub descriptor: &'a ToolDescriptor,
    pub node_id: &'a str,
    pub input: &'a Value,
    pub incoming_taint: &'a TaintSet,
    pub action_class: ActionClass,
    pub granted_offset: u64,
    pub receipt_id: &'a str,
    /// Execution mode (F6). In `Replay` the adapter is NEVER invoked — outputs
    /// are reconstructed from the durable receipt log instead.
    pub source_mode: SourceMode,
}

impl<'a> RunToolCtx<'a> {
    fn receipt(
        &self,
        phase: ToolPhase,
        output: &str,
        taint_out: &TaintSet,
        lease_id: Option<String>,
        deny_reason: Option<DenyReason>,
    ) -> ToolReceipt {
        let phase_label = serde_json::to_value(phase)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        // Defensive redaction: any output (incl. an adapter ERROR string, which
        // commonly carries stderr / auth headers) is scanned and redacted here so
        // `finalize` can never reject — no call site can trigger a panic or leak.
        // The input is already guaranteed clean (authorize's credential scan).
        let safe_output = if zyal_core::contains_any_credential(output).is_some() {
            "<redacted: credential in output>"
        } else {
            output
        };
        ToolReceipt::finalize(ReceiptInput {
            receipt_id: self.receipt_id,
            tool_id: &self.descriptor.tool_id,
            node_id: self.node_id,
            phase: &phase_label,
            latency_ms: 0,
            cost_usd: 0.0,
            input: &self.input.to_string(),
            output: safe_output,
            taint_in: self.incoming_taint,
            taint_out,
            lease_id,
            deny_reason,
        })
        .expect("input pre-scanned by authorize; output redacted above")
    }
}

/// Authorize → invoke (via the adapter) → receipt, returning the phase sequence
/// for the caller to emit. Denials and failures still produce a receipt; a tool
/// that emits a credential is flagged `misused` and its output is NOT persisted.
pub fn run_tool(ctx: &RunToolCtx, adapter: &dyn ToolAdapter) -> ToolCallOutcome {
    let mut phases = vec![ToolPhase::Queued, ToolPhase::Authorizing];
    let req = AuthorizeRequest {
        tool_id: &ctx.descriptor.tool_id,
        node_id: ctx.node_id,
        capabilities: &ctx.descriptor.capabilities,
        side_effecting: ctx.descriptor.side_effecting,
        command: None,
        url: None,
        input: ctx.input,
        incoming_taint: ctx.incoming_taint,
        action_class: ctx.action_class,
        sandbox_profile: &ctx.descriptor.sandbox_profile,
        granted_offset: ctx.granted_offset,
    };
    let lease = match authorize_tool_call(&req) {
        Ok(lease) => lease,
        Err(denied) => {
            phases.push(ToolPhase::Denied);
            let receipt = ctx.receipt(
                ToolPhase::Denied,
                "",
                ctx.incoming_taint,
                None,
                Some(denied.reason),
            );
            return ToolCallOutcome {
                phases,
                receipt,
                lease: None,
                denied: Some(denied),
                output: None,
            };
        }
    };
    phases.push(ToolPhase::Started);
    let lease_id = Some(lease.lease_id.clone());
    // Replay (F6) reconstructs tool outputs from the durable receipt log — a
    // tool is NEVER re-invoked (no network, no secrets, no double side effect).
    // This is a BACKSTOP: a correct replay caller serves the cached output from
    // the durable receipt log and never reaches here. If it does, we return a
    // placeholder receipt (phase `Cached`) that does NOT fabricate a real output
    // hash — the marker makes `output_hash` self-evidently non-authoritative,
    // and `taint_out` is a conservative over-approximation of `taint_in` (taint
    // never decreases), so the placeholder never under-reports taint. The log
    // holds the authoritative output + taint.
    if !ctx.source_mode.allows_invoke() {
        const REPLAY_PLACEHOLDER: &str =
            "<replayed: not re-invoked; authoritative output in receipt log>";
        phases.push(ToolPhase::Cached);
        let receipt = ctx.receipt(
            ToolPhase::Cached,
            REPLAY_PLACEHOLDER,
            ctx.incoming_taint,
            lease_id,
            None,
        );
        return ToolCallOutcome {
            phases,
            receipt,
            lease: Some(lease),
            denied: None,
            output: None,
        };
    }
    match adapter.invoke(&lease, ctx.input) {
        Ok(out) => {
            // A tool emitting a credential is misuse — flag it and drop the output.
            if zyal_core::contains_any_credential(&out.output).is_some() {
                phases.push(ToolPhase::Misused);
                let receipt = ctx.receipt(
                    ToolPhase::Misused,
                    "<redacted: credential in output>",
                    ctx.incoming_taint,
                    lease_id,
                    Some(DenyReason::ForbiddenContent),
                );
                return ToolCallOutcome {
                    phases,
                    receipt,
                    lease: Some(lease),
                    denied: None,
                    output: None,
                };
            }
            phases.push(ToolPhase::Succeeded);
            let receipt = ctx.receipt(
                ToolPhase::Succeeded,
                &out.output,
                ctx.incoming_taint,
                lease_id,
                None,
            );
            ToolCallOutcome {
                phases,
                receipt,
                lease: Some(lease),
                denied: None,
                output: Some(out),
            }
        }
        Err(err) => {
            phases.push(ToolPhase::Failed);
            let receipt = ctx.receipt(ToolPhase::Failed, &err, ctx.incoming_taint, lease_id, None);
            ToolCallOutcome {
                phases,
                receipt,
                lease: Some(lease),
                denied: None,
                output: None,
            }
        }
    }
}

/// Detects tool misuse across calls: repeated identical input with no progress,
/// or an error burst. The caller pauses/replans/cools-down on a hit.
#[derive(Debug)]
pub struct ToolMisuseGuard {
    last_input_hash: Option<String>,
    repeat_count: u32,
    error_count: u32,
    /// Cumulative cost ceiling; `INFINITY` (the default) never trips (M9-cont).
    cost_ceiling_usd: f64,
    cumulative_cost_usd: f64,
}

impl Default for ToolMisuseGuard {
    fn default() -> Self {
        Self {
            last_input_hash: None,
            repeat_count: 0,
            error_count: 0,
            cost_ceiling_usd: f64::INFINITY,
            cumulative_cost_usd: 0.0,
        }
    }
}

impl ToolMisuseGuard {
    /// Build a guard with a per-tool cumulative cost ceiling (M9-cont).
    pub fn with_cost_ceiling(cost_ceiling_usd: f64) -> Self {
        Self {
            cost_ceiling_usd,
            ..Default::default()
        }
    }

    /// Observe one call; returns `Some(reason)` when misuse is detected —
    /// `repeated_input_no_progress`, `error_burst`, or `cost_runaway`.
    pub fn observe(
        &mut self,
        input_hash: &str,
        errored: bool,
        cost_usd: f64,
    ) -> Option<&'static str> {
        if self.last_input_hash.as_deref() == Some(input_hash) {
            self.repeat_count += 1;
        } else {
            self.repeat_count = 0;
            self.last_input_hash = Some(input_hash.to_string());
        }
        self.error_count = if errored { self.error_count + 1 } else { 0 };
        if cost_usd.is_finite() && cost_usd > 0.0 {
            self.cumulative_cost_usd += cost_usd;
        }
        // 3 identical inputs in a row (repeat_count reaches 2) = no progress.
        if self.repeat_count >= 2 {
            return Some("repeated_input_no_progress");
        }
        if self.error_count >= 4 {
            return Some("error_burst");
        }
        if self.cumulative_cost_usd > self.cost_ceiling_usd {
            return Some("cost_runaway");
        }
        None
    }
}
