//! Generated-suite execution: runs the procedurally generated benchmark suite
//! against a `MemorySystem` adapter and serializes the full result envelope as
//! JSON. Pulled out of `runner.rs` to keep that file under the audit floor.

use crate::case::Split;
use crate::generated::{
    generate_compounding_suite, generate_hardening_suite, generate_suite, CompoundingConfig,
    GeneratedSuiteConfig, HardeningConfig,
};
use crate::runner::CandidateReport;
use crate::scoring::gates::GateFindings;
use crate::{AxisScores, BenchCase, MemorySystem, SuiteConfig};

#[path = "runner_generated_eval.rs"]
mod runner_generated_eval;
#[path = "runner_generated_metrics.rs"]
mod runner_generated_metrics;
#[path = "runner_generated_report.rs"]
mod runner_generated_report;

pub(crate) fn run_generated_candidate(
    candidate: &str,
    adapter: &mut dyn MemorySystem,
    config: &SuiteConfig,
) -> Result<CandidateReport, String> {
    match config.split {
        Split::PublicCompounding => {
            let cases = generate_compounding_suite(&CompoundingConfig {
                benchmark_version: config.benchmark_version,
                seed_label: config.seed_label.clone(),
                fixture_count: config.fixture_count,
            });
            return runner_generated_report::run_compounding_candidate(
                candidate,
                adapter,
                config,
                cases,
            );
        }
        Split::PublicHardening => {
            let cases = generate_hardening_suite(&HardeningConfig {
                benchmark_version: config.benchmark_version,
                seed_label: config.seed_label.clone(),
                fixture_count: config.fixture_count,
            });
            return runner_generated_report::run_hardening_candidate(
                candidate,
                adapter,
                config,
                cases,
            );
        }
        _ => {}
    }

    let generated_config = GeneratedSuiteConfig {
        benchmark_version: config.benchmark_version,
        split: config.split,
        seed_label: config.seed_label.clone(),
        fixture_count: config.fixture_count,
        difficulty: config.difficulty,
    };
    let cases = generate_suite(&generated_config);
    runner_generated_report::run_legacy_generated_candidate(candidate, adapter, config, &cases)
}

struct GeneratedOutcome {
    score: f32,
    axes: AxisScores,
    gates: GateFindings,
    metrics: std::collections::BTreeMap<String, crate::json::Json>,
}

fn default_gates() -> GateFindings {
    GateFindings {
        deterministic: true,
        knowledge_non_degradation: true,
        ..Default::default()
    }
}

fn empty_axes() -> AxisScores {
    AxisScores {
        correctness: f32::NAN,
        provenance: f32::NAN,
        bitemporal_recall: f32::NAN,
        contradiction: f32::NAN,
        math_science: f32::NAN,
        english_discourse_coreference: f32::NAN,
        privacy_redaction: f32::NAN,
        procedural_skill: f32::NAN,
        feedback_adaptation: f32::NAN,
        determinism_rebuild: f32::NAN,
        compounding: f32::NAN,
        topic_hardening: f32::NAN,
    }
}

fn gate_findings_json(gates: &GateFindings) -> crate::json::Json {
    crate::json::obj(&[
        ("unsafe_tool_exec", crate::json::Json::Int(gates.unsafe_tool_exec as i64)),
        ("privacy_leaks", crate::json::Json::Int(gates.privacy_leaks as i64)),
        (
            "citation_issue_count",
            crate::json::Json::Int(gates.citation_issues as i64),
        ),
        ("future_leaks", crate::json::Json::Int(gates.future_leaks as i64)),
        ("deterministic", crate::json::Json::Bool(gates.deterministic)),
        (
            "compounding_regression",
            crate::json::Json::Float(gates.compounding_regression as f64),
        ),
        (
            "hardening_regression",
            crate::json::Json::Float(gates.hardening_regression as f64),
        ),
        (
            "knowledge_non_degradation",
            crate::json::Json::Bool(gates.knowledge_non_degradation),
        ),
    ])
}

fn compounding_kind(case: &BenchCase) -> &'static str {
    compounding_kind_from_id(&case.id)
}

fn compounding_kind_from_id(id: &str) -> &'static str {
    if id.ends_with("-math") {
        "math_chain"
    } else if id.ends_with("-physics") {
        "physics_chain"
    } else if id.ends_with("-real-paper") {
        "real_paper_chain"
    } else if id.ends_with("-paper") {
        "paper_distillation"
    } else if id.ends_with("-proc") {
        "procedure_evolution"
    } else if id.ends_with("-xdom") {
        "cross_domain_transfer"
    } else if id.ends_with("-poison") {
        "poisoned_paper"
    } else {
        "unknown"
    }
}

fn compounding_depth_weight(kind: &str) -> f32 {
    match kind {
        "math_chain" => 1.0,
        "physics_chain" => 1.5,
        "paper_distillation" => 2.25,
        "procedure_evolution" => 3.4,
        "cross_domain_transfer" => 1.5,
        "poisoned_paper" => 2.25,
        "real_paper_chain" => 3.4,
        _ => 1.0,
    }
}

fn compounding_hop_depth(kind: &str) -> u32 {
    match kind {
        "math_chain" => 2,
        "physics_chain" => 2,
        "paper_distillation" => 3,
        "procedure_evolution" => 2,
        "cross_domain_transfer" => 2,
        "poisoned_paper" => 2,
        "real_paper_chain" => 4,
        _ => 1,
    }
}
