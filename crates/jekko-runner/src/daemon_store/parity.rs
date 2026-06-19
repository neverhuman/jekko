use anyhow::Result;
use jekko_store::daemon::{self, ParityCaseRow, ParityResultRow, ParityRunRow, PerfBudgetRow};
use jekko_store::db::Db;

use crate::parity_lab::{ParityArtifacts, ParityCase, ParitySummary};

use super::helpers::now_ms;

/// Persist parity cases, run summary, raw results, and perf budgets.
pub fn persist_parity_summary(
    db: &Db,
    run_id: &str,
    target_id: &str,
    cases: &[ParityCase],
    artifacts: &ParityArtifacts,
    summary: &ParitySummary,
) -> Result<String> {
    let conn = db.connection();
    let now = now_ms();
    for case in cases {
        daemon::upsert_parity_case(
            conn,
            &ParityCaseRow {
                id: case.id.clone(),
                run_id: run_id.to_string(),
                target_id: target_id.to_string(),
                tags: case.tags.clone(),
                target_kind: case.target_kind.clone(),
                steps_json: serde_json::to_value(&case.steps)?,
                perf_json: case.perf.as_ref().map(serde_json::to_value).transpose()?,
                approved: case.is_required(),
                time_created: now,
                time_updated: now,
            },
        )?;
        if let Some(max_ratio) = case.perf.as_ref().and_then(|perf| perf.p95_ms_max_ratio) {
            daemon::upsert_perf_budget(
                conn,
                &PerfBudgetRow {
                    id: format!("budget-{run_id}-{}", case.id),
                    run_id: run_id.to_string(),
                    case_id: case.id.clone(),
                    metric: "p95_ms".to_string(),
                    max_ratio: Some(max_ratio),
                    baseline_value: None,
                    candidate_value: None,
                    status: if summary.perf_over_budget == 0 {
                        "pass".to_string()
                    } else {
                        "fail".to_string()
                    },
                    time_created: now,
                    time_updated: now,
                },
            )?;
        }
    }
    let parity_run_id = format!("parity-run-{run_id}");
    daemon::upsert_parity_run(
        conn,
        &ParityRunRow {
            id: parity_run_id.clone(),
            run_id: run_id.to_string(),
            target_id: target_id.to_string(),
            case_count: cases.len() as i64,
            status: summary.status.clone(),
            report_path: Some(artifacts.summary_json.display().to_string()),
            started_at: Some(now),
            ended_at: Some(now),
            summary_json: Some(serde_json::to_value(summary)?),
            time_created: now,
            time_updated: now,
        },
    )?;
    for result in &summary.report.results {
        daemon::insert_parity_result(
            conn,
            &ParityResultRow {
                id: format!("result-{run_id}-{}-{}-{now}", result.case_id, result.target),
                parity_run_id: parity_run_id.clone(),
                case_id: result.case_id.clone(),
                target_name: result.target.clone(),
                status: result.status.clone(),
                skipped: result.skipped,
                duration_ms: result
                    .perf
                    .as_ref()
                    .and_then(|perf| perf.get("duration_ms"))
                    .and_then(serde_json::Value::as_i64)
                    .or_else(|| result.elapsed_nanos.map(|nanos| (nanos / 1_000_000) as i64)),
                perf_json: result.perf.clone(),
                message: result.message.clone(),
                time_created: now,
            },
        )?;
    }
    Ok(parity_run_id)
}
