//! Pattern compiler (M7) — the generic container model.
//!
//! A top-level `patterns:` block declares orchestration patterns as typed nodes;
//! `Builder::emit_patterns` (in `flowgraph.rs`) lowers each to concrete FlowGraph
//! IR with stable provenance (`pattern/<id>/<template_node>`), so a 100-agent
//! scatter/gather is a few lines of YAML, not hand-wired boxes.
//!
//! This first slice proves the model with two patterns:
//!   * `scatter_gather` — split → branch[] → gather/reduce,
//!   * `loop_until_dry` — body + a `dry_certificate` + a loop-back, stopping on
//!     measured marginal-utility exhaustion (not a fixed iteration count).
//!
//! This module is the pure PARSER (YAML → specs); emission lives on the Builder
//! so it shares id de-dup + edge sequencing.

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub(super) enum PatternSpec {
    ScatterGather {
        id: String,
        /// Number of branches (from `split.count` or `split.items` length).
        /// `None` → resolved at runtime (a bounded dynamic group).
        count: Option<u64>,
        /// Upper bound for the dynamic case.
        max: Option<u64>,
        branch_agent: String,
        gather_agent: String,
        reduce: String,
    },
    LoopUntilDry {
        id: String,
        body_agent: String,
        /// Safety ceiling on rounds (the loop stops on dryness, not this — but an
        /// unbounded loop is never allowed to run forever).
        max_rounds: Option<u64>,
        /// Marginal-utility threshold below which the loop is "dry".
        marginal_threshold: Option<f64>,
    },
}

impl PatternSpec {
    pub(super) fn id(&self) -> &str {
        match self {
            PatternSpec::ScatterGather { id, .. } | PatternSpec::LoopUntilDry { id, .. } => id,
        }
    }
}

/// Parse the top-level `patterns:` block. Unknown/invalid entries are skipped
/// (the structural validator owns hard errors); missing block → empty.
pub(super) fn parse_patterns(root: &Map<String, Value>) -> Vec<PatternSpec> {
    let Some(block) = root.get("patterns").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (id, spec) in block {
        let Some(spec) = spec.as_object() else {
            continue;
        };
        let kind = spec.get("kind").and_then(Value::as_str).unwrap_or("");
        match kind {
            "scatter_gather" => out.push(parse_scatter_gather(id, spec)),
            "loop_until_dry" => out.push(parse_loop_until_dry(id, spec)),
            _ => continue,
        }
    }
    // Deterministic order regardless of map iteration.
    out.sort_by(|a, b| a.id().cmp(b.id()));
    out
}

fn parse_scatter_gather(id: &str, spec: &Map<String, Value>) -> PatternSpec {
    let split = spec.get("split").and_then(Value::as_object);
    let count = split
        .and_then(|s| s.get("count"))
        .and_then(Value::as_u64)
        .or_else(|| {
            split
                .and_then(|s| s.get("items"))
                .and_then(Value::as_array)
                .map(|a| a.len() as u64)
        });
    let max = split.and_then(|s| s.get("max")).and_then(Value::as_u64);
    let branch_agent = spec
        .get("branch")
        .and_then(Value::as_object)
        .and_then(|b| b.get("agent"))
        .and_then(Value::as_str)
        .unwrap_or("worker")
        .to_string();
    let gather = spec.get("gather").and_then(Value::as_object);
    let gather_agent = gather
        .and_then(|g| g.get("agent"))
        .and_then(Value::as_str)
        .unwrap_or("reducer")
        .to_string();
    let reduce = gather
        .and_then(|g| g.get("reduce"))
        .and_then(Value::as_str)
        .unwrap_or("concat")
        .to_string();
    PatternSpec::ScatterGather {
        id: id.to_string(),
        count,
        max,
        branch_agent,
        gather_agent,
        reduce,
    }
}

/// Enforce bounded-by-construction on pattern specs (the same hard rule
/// `validate_cardinality` enforces for authored nodes — but pattern nodes are
/// synthesized, so they need their own gate). Runs before lowering.
pub(super) fn validate_patterns(specs: &[PatternSpec]) -> Result<()> {
    for spec in specs {
        match spec {
            PatternSpec::ScatterGather { id, count, max, .. } => {
                if count.is_none() && max.is_none() {
                    return Err(anyhow!(
                        "ZYAL_E_PATTERN_UNBOUNDED_SCATTER: scatter_gather `{id}` needs a `split.count` or `split.max` bound (a runtime-resolved scatter must be bounded)"
                    ));
                }
            }
            PatternSpec::LoopUntilDry { id, max_rounds, .. } => {
                if max_rounds.is_none() {
                    return Err(anyhow!(
                        "ZYAL_E_PATTERN_UNBOUNDED_LOOP: loop_until_dry `{id}` needs a `stop.max_rounds` safety ceiling (it stops on dryness, but may never run unbounded)"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn parse_loop_until_dry(id: &str, spec: &Map<String, Value>) -> PatternSpec {
    let body_agent = spec
        .get("body")
        .and_then(Value::as_object)
        .and_then(|b| b.get("agent"))
        .and_then(Value::as_str)
        .unwrap_or("refiner")
        .to_string();
    let stop = spec.get("stop").and_then(Value::as_object);
    let max_rounds = stop
        .and_then(|s| s.get("max_rounds"))
        .and_then(Value::as_u64);
    let marginal_threshold = stop
        .and_then(|s| s.get("marginal_utility_below"))
        .and_then(Value::as_f64);
    PatternSpec::LoopUntilDry {
        id: id.to_string(),
        body_agent,
        max_rounds,
        marginal_threshold,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root(patterns: Value) -> Map<String, Value> {
        json!({ "patterns": patterns }).as_object().unwrap().clone()
    }

    #[test]
    fn parses_scatter_gather_with_static_count() {
        let specs = parse_patterns(&root(json!({
            "sweep": { "kind": "scatter_gather", "split": { "count": 5 },
                       "branch": { "agent": "researcher" },
                       "gather": { "agent": "synthesizer", "reduce": "merge" } }
        })));
        assert_eq!(specs.len(), 1);
        match &specs[0] {
            PatternSpec::ScatterGather {
                id,
                count,
                branch_agent,
                reduce,
                ..
            } => {
                assert_eq!(id, "sweep");
                assert_eq!(*count, Some(5));
                assert_eq!(branch_agent, "researcher");
                assert_eq!(reduce, "merge");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn scatter_count_from_items_length() {
        let specs = parse_patterns(&root(json!({
            "s": { "kind": "scatter_gather", "split": { "items": ["a", "b", "c"] } }
        })));
        match &specs[0] {
            PatternSpec::ScatterGather { count, .. } => assert_eq!(*count, Some(3)),
            _ => panic!(),
        }
    }

    #[test]
    fn parses_loop_until_dry() {
        let specs = parse_patterns(&root(json!({
            "refine": { "kind": "loop_until_dry", "body": { "agent": "refiner" },
                        "stop": { "marginal_utility_below": 0.05, "max_rounds": 8 } }
        })));
        match &specs[0] {
            PatternSpec::LoopUntilDry {
                id,
                max_rounds,
                marginal_threshold,
                ..
            } => {
                assert_eq!(id, "refine");
                assert_eq!(*max_rounds, Some(8));
                assert_eq!(*marginal_threshold, Some(0.05));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn unknown_pattern_kinds_are_skipped_and_order_is_stable() {
        let specs = parse_patterns(&root(json!({
            "z": { "kind": "scatter_gather", "split": { "count": 1 } },
            "a": { "kind": "loop_until_dry" },
            "m": { "kind": "not_a_pattern" }
        })));
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].id(), "a"); // sorted
        assert_eq!(specs[1].id(), "z");
    }

    #[test]
    fn no_patterns_block_is_empty() {
        assert!(parse_patterns(&json!({}).as_object().unwrap().clone()).is_empty());
    }

    #[test]
    fn unbounded_scatter_is_rejected_but_bounded_passes() {
        let unbounded = parse_patterns(&root(json!({
            "s": { "kind": "scatter_gather", "branch": { "agent": "w" } }
        })));
        let err = validate_patterns(&unbounded).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_PATTERN_UNBOUNDED_SCATTER"),
            "got {err}"
        );
        // bounded by count
        validate_patterns(&parse_patterns(&root(json!({
            "s": { "kind": "scatter_gather", "split": { "count": 3 } }
        }))))
        .unwrap();
        // bounded by max (dynamic)
        validate_patterns(&parse_patterns(&root(json!({
            "s": { "kind": "scatter_gather", "split": { "max": 64 } }
        }))))
        .unwrap();
    }

    #[test]
    fn loop_without_max_rounds_is_rejected() {
        let unbounded = parse_patterns(&root(json!({
            "l": { "kind": "loop_until_dry", "body": { "agent": "r" },
                   "stop": { "marginal_utility_below": 0.05 } }
        })));
        let err = validate_patterns(&unbounded).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_PATTERN_UNBOUNDED_LOOP"),
            "got {err}"
        );
        validate_patterns(&parse_patterns(&root(json!({
            "l": { "kind": "loop_until_dry", "stop": { "max_rounds": 8 } }
        }))))
        .unwrap();
    }
}
