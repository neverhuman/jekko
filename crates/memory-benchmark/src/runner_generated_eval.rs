use std::collections::BTreeMap;

use crate::json::Json;
use crate::{BenchCase, CompoundCase, CompoundQuery, HardeningCase, MemorySystem, RecallResult};

use super::runner_generated_metrics::{
    score_compounding_case, score_generated_result, score_hardening_case,
    score_result_against_oracle,
};
use super::{default_gates, empty_axes, GeneratedOutcome};

pub(super) fn run_generated_case(
    adapter: &mut dyn MemorySystem,
    case: &BenchCase,
    budget: u32,
) -> GeneratedOutcome {
    for event in &case.events {
        let _ = adapter.observe(event);
    }
    let Some(query) = &case.query else {
        return GeneratedOutcome {
            score: 0.5,
            axes: empty_axes(),
            gates: default_gates(),
            metrics: BTreeMap::new(),
        };
    };
    let mut query = query.clone();
    query.token_budget = budget;
    match case.oracle.kind {
        crate::case::OracleKind::Hardening => score_hardening_case(adapter, case, &query),
        crate::case::OracleKind::Compounding => {
            let result = recall_case(adapter, case, &query);
            score_compounding_case(&result, case)
        }
        _ => {
            let result = recall_case(adapter, case, &query);
            let mut metrics = BTreeMap::new();
            metrics.insert(
                "base_score".to_string(),
                Json::Float(score_generated_result(&result, case) as f64),
            );
            GeneratedOutcome {
                score: score_generated_result(&result, case),
                axes: empty_axes(),
                gates: default_gates(),
                metrics,
            }
        }
    }
}

pub(super) fn run_hardening_case(
    adapter: &mut dyn MemorySystem,
    case: &HardeningCase,
    budget: u32,
) -> GeneratedOutcome {
    // 1. Ingest canonical (base) events first.
    for event in &case.base_events {
        let _ = adapter.observe(event);
    }
    // 2. Five timesteps. Between queries 0..4, observe one reinforcement event.
    //    After the final (5th) query, no reinforcement is observed.
    let mut query = case.query.clone();
    query.token_budget = budget;
    let mut metrics = BTreeMap::new();
    let mut results: Vec<RecallResult> = Vec::with_capacity(5);
    for step in 0..5 {
        let result = adapter.recall(&query);
        metrics.insert(
            format!("t{step}_used_ids"),
            Json::Array(result.used_ids.iter().cloned().map(Json::Str).collect()),
        );
        metrics.insert(
            format!("t{step}_confidence"),
            Json::Float(result.confidence as f64),
        );
        metrics.insert(
            format!("t{step}_context_tokens"),
            Json::Int(result.context_token_count as i64),
        );
        metrics.insert(
            format!("t{step}_pack_hash"),
            Json::Str(result.context_pack_hash.clone()),
        );
        results.push(result);
        if step < 4 {
            if let Some(reinforcement) = case.reinforcements.get(step) {
                let _ = adapter.observe(reinforcement);
            }
        }
    }

    // 3. Score using the closed form in score_hardening_case_results.
    let (score, axes, scored) = score_hardening_case_results(case, &results);

    // 4. Privacy leak detection via must_not_contain (mirrors run_generated_case).
    let mut privacy_leaks: u32 = 0;
    if !case.oracle.must_not_contain.is_empty() {
        for result in &results {
            let answer = result.answer.to_lowercase();
            if case
                .oracle
                .must_not_contain
                .iter()
                .any(|needle| answer.contains(&needle.to_lowercase()))
            {
                privacy_leaks = privacy_leaks.saturating_add(1);
            }
        }
    }

    // 5. Determinism for the gate uses the Phase 1 proxy:
    //    non-empty context_pack_hash at t4. Same source as the score component.
    let deterministic_gate = scored.determinism >= 1.0;

    metrics.insert(
        "all_timesteps_correct".to_string(),
        Json::Bool(scored.all_timesteps_correct),
    );
    metrics.insert(
        "support_concentration".to_string(),
        Json::Float(scored.support_concentration as f64),
    );
    metrics.insert(
        "confidence_growth".to_string(),
        Json::Float(scored.confidence_growth as f64),
    );
    metrics.insert(
        "token_reduction".to_string(),
        Json::Float(scored.token_reduction as f64),
    );
    metrics.insert(
        "determinism".to_string(),
        Json::Float(scored.determinism as f64),
    );
    metrics.insert("score".to_string(), Json::Float(score as f64));
    metrics.insert("privacy_leaks".to_string(), Json::Int(privacy_leaks as i64));

    GeneratedOutcome {
        score,
        axes,
        gates: crate::scoring::gates::GateFindings {
            deterministic: deterministic_gate,
            privacy_leaks,
            knowledge_non_degradation: scored.all_timesteps_correct,
            hardening_regression: if scored.all_timesteps_correct {
                0.0
            } else {
                1.0
            },
            ..Default::default()
        },
        metrics,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct HardeningScoreBreakdown {
    all_timesteps_correct: bool,
    support_concentration: f32,
    confidence_growth: f32,
    token_reduction: f32,
    determinism: f32,
}

/// Closed-form hardening score over exactly 5 recall results.
///
/// - Correctness gate per spec: every timestep must satisfy `must_include`
///   (in `used_ids`) and `must_contain` (in lower-cased `answer`). If any
///   timestep fails the gate, the case scores 0.0.
/// - Composite metrics (only meaningful if the gate passes):
///     - support_concentration: clamp((|used@t0| - |used@t4|) / max(1, |used@t0|), 0, 1)
///     - confidence_growth: clamp(conf@t4 - conf@t0, 0, 1)
///     - token_reduction: clamp((tok@t0 - tok@t4) / max(1, tok@t0), 0, 1)
///     - determinism: 1.0 iff context_pack_hash@t4 is non-empty, else 0.0
/// - hardening_score = 0.4*sc + 0.3*cg + 0.2*tr + 0.1*det
fn score_hardening_case_results(
    case: &HardeningCase,
    results: &[RecallResult],
) -> (f32, crate::AxisScores, HardeningScoreBreakdown) {
    let mut axes = empty_axes();
    if results.len() != 5 {
        axes.correctness = 0.0;
        axes.topic_hardening = 0.0;
        return (0.0, axes, HardeningScoreBreakdown::default());
    }

    // Correctness prerequisite at EVERY timestep.
    let all_timesteps_correct = results.iter().all(|r| {
        let used: std::collections::BTreeSet<&str> =
            r.used_ids.iter().map(String::as_str).collect();
        let include_ok = case
            .oracle
            .must_include
            .iter()
            .all(|id| used.contains(id.as_str()));
        let answer_lower = r.answer.to_lowercase();
        let contain_ok = case
            .oracle
            .must_contain
            .iter()
            .all(|needle| answer_lower.contains(&needle.to_lowercase()));
        include_ok && contain_ok
    });

    if !all_timesteps_correct {
        axes.correctness = 0.0;
        axes.topic_hardening = 0.0;
        return (
            0.0,
            axes,
            HardeningScoreBreakdown {
                all_timesteps_correct: false,
                ..Default::default()
            },
        );
    }

    let t0 = &results[0];
    let t4 = &results[4];

    let used_t0 = t0.used_ids.len() as f32;
    let used_t4 = t4.used_ids.len() as f32;
    let support_concentration = if used_t0 > 0.0 {
        ((used_t0 - used_t4) / used_t0).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let confidence_growth = (t4.confidence - t0.confidence).clamp(0.0, 1.0);

    let tok_t0 = t0.context_token_count as f32;
    let tok_t4 = t4.context_token_count as f32;
    let token_reduction = if tok_t0 > 0.0 {
        ((tok_t0 - tok_t4) / tok_t0).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let determinism = if !t4.context_pack_hash.is_empty() {
        1.0
    } else {
        0.0
    };

    let hardening_score = 0.4 * support_concentration
        + 0.3 * confidence_growth
        + 0.2 * token_reduction
        + 0.1 * determinism;

    axes.correctness = 1.0;
    axes.topic_hardening = hardening_score;

    (
        hardening_score,
        axes,
        HardeningScoreBreakdown {
            all_timesteps_correct: true,
            support_concentration,
            confidence_growth,
            token_reduction,
            determinism,
        },
    )
}

pub(super) fn run_compound_case(
    adapter: &mut dyn MemorySystem,
    case: &CompoundCase,
    budget: u32,
) -> GeneratedOutcome {
    for event in &case.events {
        let _ = adapter.observe(event);
    }
    let kind = super::compounding_kind_from_id(&case.id);
    let mut metrics = BTreeMap::new();
    let mut query_records = Vec::new();
    let mut acc = 0.0_f32;
    let mut wsum = 0.0_f32;
    let mut deterministic = true;
    let mut controls_clean = true;

    for compound_query in &case.queries {
        let mut query = compound_query.query.clone();
        query.token_budget = budget.min(query.token_budget);
        let primary = adapter.recall(&query);
        let repeat = adapter.recall(&query);
        deterministic &= stable_recall(&primary, &repeat);
        let query_score = score_result_against_oracle(&primary, &compound_query.oracle);
        acc += query_score * compound_query.depth_weight;
        wsum += compound_query.depth_weight;
        if compound_query.control && query_score < 1.0 {
            controls_clean = false;
        }
        query_records.push(compound_query_json(compound_query, &primary, query_score));
    }

    let score = if wsum > 0.0 { acc / wsum } else { 0.0 };
    let mut axes = empty_axes();
    axes.compounding = score;
    metrics.insert("fixture_kind".to_string(), Json::Str(kind.to_string()));
    metrics.insert(
        "query_count".to_string(),
        Json::Int(case.queries.len() as i64),
    );
    metrics.insert("queries".to_string(), Json::Array(query_records));
    metrics.insert("score".to_string(), Json::Float(score as f64));
    metrics.insert("controls_clean".to_string(), Json::Bool(controls_clean));
    metrics.insert("deterministic".to_string(), Json::Bool(deterministic));
    GeneratedOutcome {
        score,
        axes,
        gates: crate::scoring::gates::GateFindings {
            deterministic,
            knowledge_non_degradation: controls_clean,
            compounding_regression: if controls_clean { 0.0 } else { 1.0 },
            ..Default::default()
        },
        metrics,
    }
}

fn compound_query_json(query: &CompoundQuery, result: &RecallResult, query_score: f32) -> Json {
    crate::json::obj(&[
        ("label", Json::Str(query.label.clone())),
        ("control", Json::Bool(query.control)),
        ("hop_depth", Json::Int(query.hop_depth as i64)),
        ("depth_weight", Json::Float(query.depth_weight as f64)),
        ("score", Json::Float(query_score as f64)),
        (
            "used_ids",
            Json::Array(result.used_ids.iter().cloned().map(Json::Str).collect()),
        ),
    ])
}

pub(super) fn recall_case(
    adapter: &mut dyn MemorySystem,
    case: &BenchCase,
    query: &crate::Query,
) -> RecallResult {
    match case.lens {
        crate::TemporalLens::Current => adapter.recall(query),
        crate::TemporalLens::At => adapter.recall_at(query, case.world_time.as_deref().unwrap_or("")),
        crate::TemporalLens::AsOf => adapter.recall_as_of(query, case.tx_time.as_deref().unwrap_or("")),
        crate::TemporalLens::AtAsOf => adapter.recall_at(query, case.world_time.as_deref().unwrap_or("")),
        crate::TemporalLens::NoQuery => RecallResult::default(),
    }
}

fn stable_recall(left: &RecallResult, right: &RecallResult) -> bool {
    left.to_canonical_json() == right.to_canonical_json()
}
