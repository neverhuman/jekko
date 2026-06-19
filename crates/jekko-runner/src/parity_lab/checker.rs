//! Machine-check parity reports against declared cases.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use super::helpers::{candidate_reference_ratio, perf_ratio_for_result};
use super::types::{ParityCase, ParityReport, ParityResult, ParitySummary};

/// Machine-check a parity report.
pub fn check_report(cases: &[ParityCase], report: &ParityReport) -> Result<()> {
    let mut errors = Vec::new();
    if cases.is_empty() {
        errors.push("zero parity cases declared".to_string());
    }
    if report.results.is_empty() {
        errors.push("zero parity results reported".to_string());
    }
    for case in cases {
        let matching: Vec<&ParityResult> = report
            .results
            .iter()
            .filter(|result| result.case_id == case.id)
            .collect();
        if case.is_required() && matching.is_empty() {
            errors.push(format!("required case {} missing from report", case.id));
            continue;
        }
        for result in matching {
            if case.is_required() && result.skipped {
                errors.push(format!("required case {} was skipped", case.id));
            }
            if result.status != "passed" {
                errors.push(format!(
                    "case {} failed with status {}",
                    case.id, result.status
                ));
            }
            if case.requires_perf() && result.perf.is_none() {
                errors.push(format!("perf case {} is missing perf data", case.id));
            }
            if let Some(budget) = case.perf.as_ref().and_then(|perf| perf.p95_ms_max_ratio) {
                if perf_ratio_for_result(result).is_some_and(|ratio| ratio > budget) {
                    errors.push(format!(
                        "perf case {} ratio exceeded budget {}",
                        case.id, budget
                    ));
                }
            }
        }
        if let Some(budget) = case.perf.as_ref().and_then(|perf| perf.p95_ms_max_ratio) {
            if candidate_reference_ratio(case, report).is_some_and(|ratio| ratio > budget) {
                errors.push(format!(
                    "perf case {} candidate/reference ratio exceeded budget {}",
                    case.id, budget
                ));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(errors.join("; ")))
    }
}

/// Check a summary artifact as the parity check lane.
pub fn check_summary_artifact(path: &Path, cases: &[ParityCase]) -> Result<()> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let summary: ParitySummary = serde_json::from_str(&text).context("parse parity summary")?;
    check_report(cases, &summary.report)
}
