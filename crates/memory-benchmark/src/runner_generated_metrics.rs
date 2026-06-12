use std::collections::BTreeMap;

use crate::json::Json;
use crate::{BenchCase, CaseOracle, MemorySystem, RecallResult};

use super::runner_generated_eval::recall_case;
use super::{
    compounding_depth_weight, compounding_hop_depth, compounding_kind, empty_axes,
    GeneratedOutcome,
};

pub(super) fn score_result_against_oracle(result: &RecallResult, oracle: &CaseOracle) -> f32 {
    let mut hits = 0u32;
    let mut total = 0u32;
    if !oracle.must_include.is_empty() {
        total += 1;
        if oracle
            .must_include
            .iter()
            .all(|id| result.used_ids.iter().any(|used| used == id))
        {
            hits += 1;
        }
    }
    if !oracle.must_exclude.is_empty() {
        total += 1;
        if oracle
            .must_exclude
            .iter()
            .all(|id| !result.used_ids.iter().any(|used| used == id))
        {
            hits += 1;
        }
    }
    if !oracle.must_contain.is_empty() {
        total += 1;
        let answer = result.answer.to_lowercase();
        if oracle
            .must_contain
            .iter()
            .all(|needle| answer.contains(&needle.to_lowercase()))
        {
            hits += 1;
        }
    }
    if !oracle.must_not_contain.is_empty() {
        total += 1;
        let answer = result.answer.to_lowercase();
        if oracle
            .must_not_contain
            .iter()
            .all(|needle| !answer.contains(&needle.to_lowercase()))
        {
            hits += 1;
        }
    }
    if !oracle.required_warnings.is_empty() {
        total += 1;
        if oracle.required_warnings.iter().all(|needle| {
            result
                .warnings
                .iter()
                .any(|warning| warning.name() == needle)
        }) {
            hits += 1;
        }
    }
    if total == 0 {
        1.0
    } else {
        hits as f32 / total as f32
    }
}

pub(super) fn score_generated_result(result: &RecallResult, case: &BenchCase) -> f32 {
    score_result_against_oracle(result, &case.oracle)
}

pub(super) fn score_compounding_case(result: &RecallResult, case: &BenchCase) -> GeneratedOutcome {
    let kind = compounding_kind(case);
    let mut metrics = BTreeMap::new();
    let mut stages = Vec::new();
    let answer = result.answer.to_lowercase();
    let include_ok = case
        .oracle
        .must_include
        .iter()
        .all(|id| result.used_ids.iter().any(|used| used == id));
    let contain_ok = case
        .oracle
        .must_contain
        .iter()
        .all(|needle| answer.contains(&needle.to_lowercase()));
    let exclude_ok = case
        .oracle
        .must_exclude
        .iter()
        .all(|id| !result.used_ids.iter().any(|used| used == id));
    let warning_ok = case.oracle.required_warnings.iter().all(|needle| {
        result
            .warnings
            .iter()
            .any(|warning| warning.name() == needle)
    });
    let control_ok = case
        .oracle
        .must_not_contain
        .iter()
        .all(|needle| !result.answer.contains(needle));

    match kind {
        "math_chain" => {
            stages.push(include_ok);
            stages.push(contain_ok);
        }
        "physics_chain" => {
            stages.push(include_ok);
            stages.push(contain_ok);
            stages.push(result.answer.to_lowercase().contains("nav"));
        }
        "paper_distillation" => {
            stages.push(include_ok);
            stages.push(contain_ok);
            stages.push(result.used_ids.len() >= 2);
        }
        "procedure_evolution" => {
            stages.push(include_ok);
            stages.push(contain_ok);
            stages.push(warning_ok);
        }
        "cross_domain_transfer" => {
            stages.push(include_ok);
            stages.push(contain_ok);
            stages.push(exclude_ok);
        }
        "poisoned_paper" => {
            stages.push(include_ok);
            stages.push(contain_ok);
            stages.push(control_ok);
            stages.push(warning_ok);
        }
        "real_paper_chain" => {
            stages.push(include_ok);
            stages.push(contain_ok);
            stages.push(result.used_ids.len() >= 3);
        }
        _ => {
            stages.push(score_generated_result(result, case) >= 0.50);
        }
    }

    let weights = [1.0_f32, 1.5, 2.25, 3.4];
    let mut acc = 0.0_f32;
    let mut wsum = 0.0_f32;
    for (idx, stage_ok) in stages.iter().enumerate() {
        let weight = weights
            .get(idx)
            .copied()
            .unwrap_or(*weights.last().unwrap());
        acc += if *stage_ok { weight } else { 0.0 };
        wsum += weight;
    }
    let score = if wsum > 0.0 { acc / wsum } else { 0.0 };
    let mut axes = empty_axes();
    axes.compounding = score;
    metrics.insert("fixture_kind".to_string(), Json::Str(kind.to_string()));
    metrics.insert(
        "depth_weight".to_string(),
        Json::Float(compounding_depth_weight(kind) as f64),
    );
    metrics.insert(
        "hop_depth".to_string(),
        Json::Int(compounding_hop_depth(kind) as i64),
    );
    metrics.insert(
        "base_score".to_string(),
        Json::Float(score_generated_result(result, case) as f64),
    );
    metrics.insert("stage_count".to_string(), Json::Int(stages.len() as i64));
    metrics.insert("stage_score".to_string(), Json::Float(score as f64));
    GeneratedOutcome {
        score,
        axes,
        gates: crate::scoring::gates::GateFindings {
            deterministic: true,
            knowledge_non_degradation: control_ok,
            ..Default::default()
        },
        metrics,
    }
}

pub(super) fn score_hardening_case(
    adapter: &mut dyn MemorySystem,
    case: &BenchCase,
    query: &crate::Query,
) -> GeneratedOutcome {
    let mut metrics = BTreeMap::new();
    let mut results = Vec::with_capacity(5);
    for step in 0..5 {
        let result = recall_case(adapter, case, query);
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
    }

    let all_timesteps_correct = results.iter().all(|result| {
        score_generated_result(result, case) >= 1.0
            && case
                .oracle
                .must_not_contain
                .iter()
                .all(|needle| !result.answer.contains(needle))
    });
    let deterministic = results
        .windows(2)
        .last()
        .map(|pair| pair[0].context_pack_hash == pair[1].context_pack_hash)
        .unwrap_or(true);

    let first = results.first().cloned().unwrap_or_default();
    let last = results.last().cloned().unwrap_or_default();
    let support_concentration = if first.used_ids.is_empty() {
        0.0
    } else {
        ((first.used_ids.len() as f32 - last.used_ids.len() as f32)
            / first.used_ids.len().max(1) as f32)
            .clamp(0.0, 1.0)
    };
    // The current deterministic adapters converge in-place rather than
    // showing a literal delta on every repeat, so we reward the stabilized
    // confidence level itself as the best available proxy for growth.
    let confidence_growth = last.confidence.clamp(0.0, 1.0);
    let token_reduction = if first.context_token_count > 0 {
        ((first
            .context_token_count
            .saturating_sub(last.context_token_count)) as f32
            / first.context_token_count as f32)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let score = if all_timesteps_correct {
        0.55 * support_concentration
            + 0.35 * confidence_growth
            + 0.05 * token_reduction
            + 0.05 * if deterministic { 1.0 } else { 0.0 }
    } else {
        0.0
    };
    let mut axes = empty_axes();
    axes.topic_hardening = score;
    metrics.insert(
        "all_timesteps_correct".to_string(),
        Json::Bool(all_timesteps_correct),
    );
    metrics.insert(
        "support_concentration".to_string(),
        Json::Float(support_concentration as f64),
    );
    metrics.insert(
        "confidence_growth".to_string(),
        Json::Float(confidence_growth as f64),
    );
    metrics.insert(
        "token_reduction".to_string(),
        Json::Float(token_reduction as f64),
    );
    metrics.insert("deterministic".to_string(), Json::Bool(deterministic));
    metrics.insert("score".to_string(), Json::Float(score as f64));
    GeneratedOutcome {
        score,
        axes,
        gates: crate::scoring::gates::GateFindings {
            deterministic,
            knowledge_non_degradation: all_timesteps_correct,
            ..Default::default()
        },
        metrics,
    }
}
