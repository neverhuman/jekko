//! Convert parity failures into typed follow-up gaps and tasks.

use crate::port::{MasterTaskStatus, PortMasterTask};

use super::helpers::{candidate_reference_ratio, perf_ratio_for_result};
use super::types::{ParityCase, ParityGap, ParityReport, ParityResult};

/// Generate follow-up gaps from a report.
pub fn generate_gaps(cases: &[ParityCase], report: &ParityReport) -> Vec<ParityGap> {
    let mut gaps = Vec::new();
    if cases.is_empty() {
        gaps.push(gap(
            "zero-cases",
            "suite",
            "missing_case",
            "parity",
            1,
            "zero parity cases declared",
        ));
        return gaps;
    }
    for case in cases {
        let matching: Vec<&ParityResult> = report
            .results
            .iter()
            .filter(|result| result.case_id == case.id)
            .collect();
        if case.is_required() && matching.is_empty() {
            gaps.push(gap(
                &format!("missing-{}", case.id),
                &case.id,
                "missing_required",
                "correctness",
                1,
                "required parity case missing from report",
            ));
            continue;
        }
        for result in matching {
            if case.is_required() && result.skipped {
                gaps.push(gap(
                    &format!("skipped-{}-{}", case.id, result.target),
                    &case.id,
                    "skipped_required",
                    "correctness",
                    1,
                    "required parity case was skipped",
                ));
            }
            if result.status != "passed" {
                gaps.push(gap(
                    &format!("failed-{}-{}", case.id, result.target),
                    &case.id,
                    "failed_case",
                    "correctness",
                    1,
                    result.message.as_deref().unwrap_or("case failed"),
                ));
            }
            if case.requires_perf() && result.perf.is_none() {
                gaps.push(gap(
                    &format!("missing-perf-{}-{}", case.id, result.target),
                    &case.id,
                    "missing_perf",
                    "performance",
                    2,
                    "required perf case is missing perf data",
                ));
            }
            if let Some(budget) = case.perf.as_ref().and_then(|perf| perf.p95_ms_max_ratio) {
                if perf_ratio_for_result(result).is_some_and(|ratio| ratio > budget) {
                    gaps.push(gap(
                        &format!("perf-{}-{}", case.id, result.target),
                        &case.id,
                        "perf_budget",
                        "performance",
                        1,
                        &format!("latency ratio exceeded budget {budget}"),
                    ));
                }
            }
        }
        if let Some(budget) = case.perf.as_ref().and_then(|perf| perf.p95_ms_max_ratio) {
            if candidate_reference_ratio(case, report).is_some_and(|ratio| ratio > budget) {
                gaps.push(gap(
                    &format!("perf-ratio-{}", case.id),
                    &case.id,
                    "perf_budget",
                    "performance",
                    1,
                    &format!("candidate/reference p95 ratio exceeded budget {budget}"),
                ));
            }
        }
    }
    gaps
}

/// Convert a parity gap into a queued follow-up port task.
pub fn parity_gap_to_followup_task(gap: &ParityGap, stage_id: &str) -> PortMasterTask {
    PortMasterTask {
        id: format!("task-parity-gap-{}", sanitize_task_id(&gap.id)),
        stage_id: stage_id.to_string(),
        title: format!("Close parity gap {}: {}", gap.case_id, gap.category),
        task_kind: "parity_gap".to_string(),
        risk_level: if gap.priority <= 1 {
            "high".to_string()
        } else {
            "medium".to_string()
        },
        write_scope: vec!["src/**".to_string(), "tests/**".to_string()],
        bounded_write_scope: true,
        dependencies: Vec::new(),
        proof_lane: "rtk just zyal-port-fast".to_string(),
        done_evidence: vec![
            "parity/raw.jsonl".to_string(),
            "parity/summary.json".to_string(),
            "parity/gaps.json".to_string(),
        ],
        memory_scope: "run".to_string(),
        generated_zone_boundary_checks: true,
        status: MasterTaskStatus::Queued,
    }
}

fn gap(
    id: &str,
    case_id: &str,
    category: &str,
    profile: &str,
    priority: u8,
    message: &str,
) -> ParityGap {
    ParityGap {
        id: id.to_string(),
        case_id: case_id.to_string(),
        category: category.to_string(),
        profile: profile.to_string(),
        priority,
        message: message.to_string(),
        follow_up_task: serde_json::json!({
            "title": format!("Close parity gap {case_id}: {category}"),
            "category": category,
            "profile": profile,
            "priority": priority,
        }),
    }
}

fn sanitize_task_id(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "gap".to_string()
    } else {
        out
    }
}

#[cfg(test)]
pub(super) fn make_gap_for_tests(
    id: &str,
    case_id: &str,
    category: &str,
    profile: &str,
    priority: u8,
    message: &str,
) -> ParityGap {
    gap(id, case_id, category, profile, priority, message)
}
