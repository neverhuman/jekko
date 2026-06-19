use std::collections::BTreeMap;

use serde_json::Value;

use super::types::{RunSummary, SignalRow};

pub(super) fn canonical_signal_table(
    counts: &BTreeMap<&'static str, u64>,
    evidence: &BTreeMap<&'static str, Value>,
) -> Vec<SignalRow> {
    // Order matches the 12 canonical signals in OBSERVABILITY.md + new ones.
    let order: &[(&str, &str)] = &[
        ("1", "model_attempt_outcome_burst"),
        ("2", "balancer_no_rotation"),
        ("3", "parity_gap_open_growth"),
        ("4", "worker_stall_or_quarantine"),
        ("5", "live_budget_exhaustion"),
        ("6", "proof_failed_in_live_lane"),
        ("7", "provider_error_rate_explosion"),
        ("8", "latency_outlier_per_provider"),
        ("9", "jankurai_regression"),
        ("10", "heartbeat_silence"),
        ("11", "parity_result_no_evidence"),
        ("12", "judge_patch_without_proof"),
        ("empty_response_streak", "empty_response_streak"),
        ("proof_passed", "proof_passed"),
        ("parity_result", "parity_result"),
        ("parity_gap", "parity_gap"),
        ("judge_patch", "judge_patch"),
        ("promotion_decision", "promotion_decision"),
        ("remediation_triggered", "remediation_triggered"),
    ];
    let mut out = Vec::with_capacity(order.len());
    for (id, name) in order {
        let count = *counts.get(name).unwrap_or(&0);
        let evidence = evidence.get(name).cloned();
        out.push(SignalRow {
            id: id.to_string(),
            name: name.to_string(),
            count,
            evidence,
        });
    }
    out
}

pub(super) fn recommend_next_steps(summary: &RunSummary) -> Vec<String> {
    let mut steps = Vec::new();
    if let Some(halt) = &summary.halt_reason {
        match halt.kind.as_str() {
            "empty_response_streak" => {
                steps.push(format!(
                    "Declare `quality_band: top20` on the `{}` stage's model_policy - \
                     see docs/ZYAL/MODEL_QUALITY_BAND.md.",
                    halt.stage.as_deref().unwrap_or("affected")
                ));
            }
            "budget_exhausted" => {
                steps.push(
                    "Raise live_call_budget.max_calls in the run's manifest, or split work \
                     across multiple runs."
                        .to_string(),
                );
            }
            "final_block" => {
                steps.push(
                    "Inspect events.jsonl filter \
                     `select(.kind==\"model_attempt_outcome\" and .data.state==\"final_block\")` \
                     for the failing stage; consider raising JEKKO_MODEL_CALL_TIMEOUT_SECS."
                        .to_string(),
                );
            }
            "parity_gap" => {
                steps.push(
                    "Inspect parity/gaps.json and route remaining gaps into explicit follow-up \
                     port tasks before rerunning signoff."
                        .to_string(),
                );
            }
            _ => {}
        }
    }
    if summary.gates.get("jankurai_gate").map(String::as_str) == Some("failed") {
        steps.push(
            "Jankurai audit regressed mid-run. Re-run audit + fix the new finding \
             before re-attempting."
                .to_string(),
        );
    }
    steps
}

pub(super) struct EmptyStreakWitness {
    pub(super) stage: Option<String>,
    pub(super) count: usize,
    pub(super) providers: std::collections::BTreeSet<String>,
    pub(super) users: std::collections::BTreeSet<String>,
    pub(super) first_ts: Option<u64>,
    pub(super) last_ts: Option<u64>,
}

impl EmptyStreakWitness {
    pub(super) fn new(stage: Option<String>) -> Self {
        Self {
            stage,
            count: 0,
            providers: std::collections::BTreeSet::new(),
            users: std::collections::BTreeSet::new(),
            first_ts: None,
            last_ts: None,
        }
    }

    pub(super) fn observe(
        &mut self,
        stage: Option<String>,
        provider: Option<String>,
        user: Option<String>,
        ts: Option<u64>,
    ) {
        if self.stage.is_none() {
            self.stage = stage;
        }
        self.count += 1;
        if let Some(p) = provider {
            self.providers.insert(p);
        }
        if let Some(u) = user {
            self.users.insert(u);
        }
        if self.first_ts.is_none() {
            self.first_ts = ts;
        }
        self.last_ts = ts;
    }

    pub(super) fn reset(&mut self) {
        self.count = 0;
        self.providers.clear();
        self.users.clear();
        self.first_ts = None;
        self.last_ts = None;
    }
}
