//! Internal helpers shared across parity submodules.

use std::time::{SystemTime, UNIX_EPOCH};

use super::types::{ParityCase, ParityReport, ParityResult};

pub(super) fn perf_payload(
    case: &ParityCase,
    elapsed_ms: u64,
    elapsed_nanos: u128,
) -> Option<serde_json::Value> {
    case.requires_perf().then(|| {
        serde_json::json!({
            "duration_ms": elapsed_ms,
            "p95_ms": elapsed_ms.max(1),
            "elapsed_nanos": elapsed_nanos,
            "captured_at": now_secs(),
        })
    })
}

pub(super) fn perf_ratio_for_result(result: &ParityResult) -> Option<f64> {
    result
        .latency_ratio
        .or_else(|| result.perf.as_ref()?.get("latency_ratio")?.as_f64())
        .or_else(|| result.perf.as_ref()?.get("p95_ms_ratio")?.as_f64())
}

pub(super) fn candidate_reference_ratio(case: &ParityCase, report: &ParityReport) -> Option<f64> {
    let reference = report
        .results
        .iter()
        .find(|result| result.case_id == case.id && result.target == report.reference)
        .and_then(p95_ms)?;
    let candidate = report
        .results
        .iter()
        .find(|result| result.case_id == case.id && result.target == report.candidate)
        .and_then(p95_ms)?;
    if reference <= 0.0 {
        None
    } else {
        Some(candidate / reference)
    }
}

fn p95_ms(result: &ParityResult) -> Option<f64> {
    result.perf.as_ref()?.get("p95_ms")?.as_f64()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
