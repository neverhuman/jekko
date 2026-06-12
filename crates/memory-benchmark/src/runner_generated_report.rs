use std::collections::BTreeMap;

use crate::memory_api::axes_to_json;
use crate::runner::CandidateReport;
use crate::runner_support::GATE_REPLAY_CMD;
use crate::scoring::gates::GateFindings;
use crate::{AxisScores, BenchCase, CompoundCase, HardeningCase, MemorySystem, SuiteConfig};

use crate::json::{self, Json};

use super::runner_generated_eval::{run_compound_case, run_generated_case, run_hardening_case};
use super::gate_findings_json;

pub(super) fn run_legacy_generated_candidate(
    candidate: &str,
    adapter: &mut dyn MemorySystem,
    config: &SuiteConfig,
    cases: &[BenchCase],
) -> Result<CandidateReport, String> {
    let mut fixture_records = Vec::with_capacity(cases.len());
    let mut scores = Vec::with_capacity(cases.len());
    let mut axis_totals = AxisScores::default();
    let mut axis_counts = AxisScores::default();
    let mut passed = 0u32;
    let mut gate_totals = GateFindings {
        deterministic: true,
        knowledge_non_degradation: true,
        ..Default::default()
    };

    for case in cases {
        let outcome = run_generated_case(adapter, case, config.context_budget);
        let score = outcome.score;
        merge_gates(&mut gate_totals, &outcome.gates);
        if score >= 0.50 {
            passed += 1;
        }
        scores.push(score);
        crate::runner_support::accumulate(&mut axis_totals, &mut axis_counts, &outcome.axes);
        let mut record = BTreeMap::new();
        record.insert("id".to_string(), Json::Str(case.id.clone()));
        record.insert(
            "block".to_string(),
            Json::Str(case.block.name().to_string()),
        );
        record.insert(
            "domain".to_string(),
            Json::Str(case.domain.name().to_string()),
        );
        record.insert(
            "oracle".to_string(),
            Json::Str(format!("{:?}", case.oracle.kind)),
        );
        record.insert("weighted".to_string(), Json::Float(score as f64));
        record.insert("axes".to_string(), axes_to_json(&outcome.axes));
        record.insert(
            "gate_findings".to_string(),
            gate_findings_json(&outcome.gates),
        );
        record.insert("metrics".to_string(), Json::Object(outcome.metrics));
        fixture_records.push(Json::Object(record));
    }

    finish_generated_report(
        candidate,
        config,
        scores,
        axis_totals,
        axis_counts,
        passed,
        gate_totals,
        fixture_records,
        None,
    )
}

pub(super) fn run_hardening_candidate(
    candidate: &str,
    adapter: &mut dyn MemorySystem,
    config: &SuiteConfig,
    cases: Vec<HardeningCase>,
) -> Result<CandidateReport, String> {
    let mut fixture_records = Vec::with_capacity(cases.len());
    let mut scores = Vec::with_capacity(cases.len());
    let mut axis_totals = AxisScores::default();
    let mut axis_counts = AxisScores::default();
    let mut passed = 0u32;
    let mut gate_totals = GateFindings {
        deterministic: true,
        knowledge_non_degradation: true,
        ..Default::default()
    };

    for case in &cases {
        let outcome = run_hardening_case(adapter, case, config.context_budget);
        merge_gates(&mut gate_totals, &outcome.gates);
        if outcome.score >= 0.50 {
            passed += 1;
        }
        scores.push(outcome.score);
        crate::runner_support::accumulate(&mut axis_totals, &mut axis_counts, &outcome.axes);
        let mut record = BTreeMap::new();
        record.insert("id".to_string(), Json::Str(case.id.clone()));
        record.insert("subject".to_string(), Json::Str(case.subject.clone()));
        record.insert("block".to_string(), Json::Str("hardening".to_string()));
        record.insert("domain".to_string(), Json::Str("science".to_string()));
        record.insert(
            "oracle".to_string(),
            Json::Str(format!("{:?}", case.oracle.kind)),
        );
        record.insert("weighted".to_string(), Json::Float(outcome.score as f64));
        record.insert("axes".to_string(), axes_to_json(&outcome.axes));
        record.insert(
            "gate_findings".to_string(),
            gate_findings_json(&outcome.gates),
        );
        record.insert("metrics".to_string(), Json::Object(outcome.metrics));
        fixture_records.push(Json::Object(record));
    }

    finish_generated_report(
        candidate,
        config,
        scores,
        axis_totals,
        axis_counts,
        passed,
        gate_totals,
        fixture_records,
        None,
    )
}

pub(super) fn run_compounding_candidate(
    candidate: &str,
    adapter: &mut dyn MemorySystem,
    config: &SuiteConfig,
    cases: Vec<CompoundCase>,
) -> Result<CandidateReport, String> {
    let mut fixture_records = Vec::with_capacity(cases.len());
    let mut scores = Vec::with_capacity(cases.len());
    let mut axis_totals = AxisScores::default();
    let mut axis_counts = AxisScores::default();
    let mut passed = 0u32;
    let mut gate_totals = GateFindings {
        deterministic: true,
        knowledge_non_degradation: true,
        ..Default::default()
    };
    let mut kind_scores: BTreeMap<String, (f32, u32)> = BTreeMap::new();

    for case in &cases {
        let outcome = run_compound_case(adapter, case, config.context_budget);
        merge_gates(&mut gate_totals, &outcome.gates);
        if outcome.score >= 0.50 {
            passed += 1;
        }
        scores.push(outcome.score);
        crate::runner_support::accumulate(&mut axis_totals, &mut axis_counts, &outcome.axes);
        if let Some(Json::Str(kind)) = outcome.metrics.get("fixture_kind") {
            let entry = kind_scores.entry(kind.clone()).or_insert((0.0, 0));
            entry.0 += outcome.score;
            entry.1 += 1;
        }
        let mut record = BTreeMap::new();
        record.insert("id".to_string(), Json::Str(case.id.clone()));
        record.insert(
            "block".to_string(),
            Json::Str(case.block.name().to_string()),
        );
        record.insert(
            "domain".to_string(),
            Json::Str(case.domain.name().to_string()),
        );
        record.insert("oracle".to_string(), Json::Str("Compounding".to_string()));
        record.insert("weighted".to_string(), Json::Float(outcome.score as f64));
        record.insert("axes".to_string(), axes_to_json(&outcome.axes));
        record.insert(
            "gate_findings".to_string(),
            gate_findings_json(&outcome.gates),
        );
        record.insert("metrics".to_string(), Json::Object(outcome.metrics));
        fixture_records.push(Json::Object(record));
    }

    let mut metrics_by_kind = BTreeMap::new();
    for (kind, (sum, count)) in kind_scores {
        metrics_by_kind.insert(
            kind,
            json::obj(&[
                ("fixtures", Json::Int(count as i64)),
                (
                    "mean_score",
                    Json::Float((sum / count.max(1) as f32) as f64),
                ),
            ]),
        );
    }

    finish_generated_report(
        candidate,
        config,
        scores,
        axis_totals,
        axis_counts,
        passed,
        gate_totals,
        fixture_records,
        Some(Json::Object(metrics_by_kind)),
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_generated_report(
    candidate: &str,
    config: &SuiteConfig,
    scores: Vec<f32>,
    axis_totals: AxisScores,
    axis_counts: AxisScores,
    passed: u32,
    gate_totals: GateFindings,
    fixture_records: Vec<Json>,
    kind_metrics: Option<Json>,
) -> Result<CandidateReport, String> {
    let raw_total = if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f32>() / scores.len() as f32 * 100.0
    };
    let ci = crate::scoring::bootstrap::bootstrap_ci(&scores, &config.seed_label, 1000);
    let avg_axes = crate::runner_support::average(&axis_totals, &axis_counts);
    let total = crate::scoring::gates::apply_hard_gates(raw_total, &gate_totals);

    let mut top = BTreeMap::new();
    top.insert("name".to_string(), Json::Str(candidate.to_string()));
    top.insert(
        "suite".to_string(),
        Json::Str(config.split.name().to_string()),
    );
    top.insert(
        "seed_label".to_string(),
        Json::Str(config.seed_label.clone()),
    );
    top.insert("total".to_string(), Json::Float(total as f64));
    top.insert("raw_total".to_string(), Json::Float(raw_total as f64));
    top.insert("axes".to_string(), axes_to_json(&avg_axes));
    top.insert("fixtures_run".to_string(), Json::Int(scores.len() as i64));
    top.insert("fixtures_passed".to_string(), Json::Int(passed as i64));
    top.insert("fixtures".to_string(), Json::Array(fixture_records));
    if let Some(kind_metrics) = kind_metrics {
        top.insert("kind_metrics".to_string(), kind_metrics);
    }
    top.insert(
        "bootstrap_ci".to_string(),
        json::obj(&[
            ("mean", Json::Float(ci.mean as f64)),
            ("ci95_low", Json::Float(ci.ci95_low as f64)),
            ("ci95_high", Json::Float(ci.ci95_high as f64)),
            ("overfit_gap", Json::Float(0.0)),
        ]),
    );
    top.insert(
        "gate_findings".to_string(),
        json::obj(&[
            (
                "unsafe_tool_exec",
                Json::Int(gate_totals.unsafe_tool_exec as i64),
            ),
            ("privacy_leaks", Json::Int(gate_totals.privacy_leaks as i64)),
            (
                "citation_issue_count",
                Json::Int(gate_totals.citation_issues as i64),
            ),
            ("future_leaks", Json::Int(gate_totals.future_leaks as i64)),
            ("deterministic", Json::Bool(gate_totals.deterministic)),
            (
                "compounding_regression",
                Json::Float(gate_totals.compounding_regression as f64),
            ),
            (
                "hardening_regression",
                Json::Float(gate_totals.hardening_regression as f64),
            ),
            (
                "knowledge_non_degradation",
                Json::Bool(gate_totals.knowledge_non_degradation),
            ),
            ("replay_cmd", Json::Str(GATE_REPLAY_CMD.to_string())),
            (
                "evidence_artifact",
                Json::Str(".jankurai/repo-score.md".to_string()),
            ),
        ]),
    );
    let json = Json::Object(top).to_string();
    Ok(CandidateReport {
        name: candidate.to_string(),
        total,
        fixtures_run: scores.len() as u32,
        fixtures_passed: passed,
        json,
    })
}

fn merge_gates(total: &mut GateFindings, gates: &GateFindings) {
    total.unsafe_tool_exec += gates.unsafe_tool_exec;
    total.privacy_leaks += gates.privacy_leaks;
    total.citation_issues += gates.citation_issues;
    total.future_leaks += gates.future_leaks;
    total.deterministic &= gates.deterministic;
    total.compounding_regression = total
        .compounding_regression
        .max(gates.compounding_regression);
    total.hardening_regression = total.hardening_regression.max(gates.hardening_regression);
    total.knowledge_non_degradation &= gates.knowledge_non_degradation;
}
