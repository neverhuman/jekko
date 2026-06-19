//! Write RedlineDB-style parity artifacts and summary payloads.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use super::checker::check_report;
use super::gaps::generate_gaps;
use super::types::{
    GeneratedParityCase, GeneratedParityManifest, ParityArtifacts, ParityCase, ParityReport,
    ParitySummary, RawParityRow,
};

/// Write RedlineDB-style raw JSONL and summary JSON artifacts.
pub fn write_report_artifacts(
    repo_root: &Path,
    run_id: &str,
    cases: &[ParityCase],
    report: ParityReport,
) -> Result<ParityArtifacts> {
    let dir = repo_root.join("target/zyal/parity").join(run_id);
    fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    let generated_manifest_json = dir.join("generated_manifest.json");
    let approved_ci_txt = dir.join("approved-ci.txt");
    let raw_jsonl = dir.join("raw.jsonl");
    let summary_json = dir.join("summary.json");
    let gaps_json = dir.join("gaps.json");
    let mut raw = fs::File::create(&raw_jsonl)?;
    for result in &report.results {
        let row = RawParityRow {
            schema_version: "zyal.parity.raw.v1".to_string(),
            case_id: result.case_id.clone(),
            target: result.target.clone(),
            status: result.status.clone(),
            skipped: result.skipped,
            exit_code: result.exit_code,
            elapsed_nanos: result.elapsed_nanos,
            stdout_sha256: result.stdout_sha256.clone(),
            stderr_sha256: result.stderr_sha256.clone(),
            perf: result.perf.clone(),
            message: result.message.clone(),
        };
        writeln!(raw, "{}", serde_json::to_string(&row)?)?;
    }
    let summary = summarize_report(cases, report);
    let manifest = generated_manifest(run_id, cases);
    fs::write(
        &generated_manifest_json,
        serde_json::to_string_pretty(&manifest)?,
    )?;
    fs::write(&approved_ci_txt, approved_ci_text(cases))?;
    fs::write(&summary_json, serde_json::to_string_pretty(&summary)?)?;
    fs::write(&gaps_json, serde_json::to_string_pretty(&summary.gaps)?)?;
    Ok(ParityArtifacts {
        generated_manifest_json,
        approved_ci_txt,
        raw_jsonl,
        summary_json,
        gaps_json,
    })
}

/// Build a Redline-style generated manifest.
pub fn generated_manifest(run_id: &str, cases: &[ParityCase]) -> GeneratedParityManifest {
    GeneratedParityManifest {
        schema_version: "zyal.parity.generated_manifest.v1".to_string(),
        run_id: run_id.to_string(),
        case_count: cases.len(),
        cases: cases
            .iter()
            .map(|case| GeneratedParityCase {
                id: case.id.clone(),
                target_kind: case.target_kind.clone(),
                tags: case.tags.clone(),
                approved: case.is_required(),
                step_count: case.steps.len(),
                requires_perf: case.requires_perf(),
            })
            .collect(),
    }
}

/// Build a summary payload.
pub fn summarize_report(cases: &[ParityCase], report: ParityReport) -> ParitySummary {
    let passed = report
        .results
        .iter()
        .filter(|result| result.status == "passed" && !result.skipped)
        .count();
    let failed = report
        .results
        .iter()
        .filter(|result| result.status != "passed")
        .count();
    let skipped = report
        .results
        .iter()
        .filter(|result| result.skipped)
        .count();
    let missing_perf = cases
        .iter()
        .filter(|case| case.requires_perf())
        .filter(|case| {
            report
                .results
                .iter()
                .filter(|result| result.case_id == case.id)
                .any(|result| result.perf.is_none())
        })
        .count();
    let gaps = generate_gaps(cases, &report);
    let perf_over_budget = gaps
        .iter()
        .filter(|gap| gap.category == "perf_budget")
        .count();
    let status = if check_report(cases, &report).is_ok() {
        "passed"
    } else {
        "failed"
    };
    ParitySummary {
        schema_version: "zyal.parity.summary.v1".to_string(),
        status: status.to_string(),
        case_count: cases.len(),
        passed,
        failed,
        skipped,
        missing_perf,
        perf_over_budget,
        gaps,
        report,
    }
}

fn approved_ci_text(cases: &[ParityCase]) -> String {
    let mut ids: Vec<&str> = cases
        .iter()
        .filter(|case| case.is_required())
        .map(|case| case.id.as_str())
        .collect();
    ids.sort_unstable();
    if ids.is_empty() {
        String::new()
    } else {
        format!("{}\n", ids.join("\n"))
    }
}
