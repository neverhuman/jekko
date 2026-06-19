//! Macro-stage plan drafting and validation for super-reasoning runs.
//!
//! Houses the canonical 9-12 stage template table, the
//! [`draft_super_master_plan`] / [`draft_super_master_plan_with_config`]
//! builders, and [`validate_super_macro_plan`] (with its private cycle
//! checker).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};

use crate::port::{
    MasterTaskStatus, PhaseStatus, PortMasterPlan, PortMasterTask, PortStage, PortTargetRequest,
};

use super::{SuperReasoningConfig, SUPER_STAGE_MAX, SUPER_STAGE_MIN};

struct StageTemplate {
    slug: &'static str,
    name: &'static str,
    objective: &'static str,
    depends_on: &'static [&'static str],
}

/// Inline mirror of `jekko_runtime::daemon::super_reasoning::canonical_phases()`.
///
/// The runtime function is private and lives in a sibling crate, so the names
/// are kept in sync here. If the canonical list shifts upstream, update this
/// table and the runtime in tandem.
fn canonical_stage_templates() -> Vec<StageTemplate> {
    vec![
        StageTemplate {
            slug: "source_of_truth",
            name: "Source of truth",
            objective: "Build the non-negotiable behavior, API, compatibility, and evidence ledger.",
            depends_on: &[],
        },
        StageTemplate {
            slug: "architecture_blueprint",
            name: "Architecture blueprint",
            objective: "Convert requirements into module boundaries, invariants, risk register, and implementation slices.",
            depends_on: &["source_of_truth"],
        },
        StageTemplate {
            slug: "repo_graph_bootstrap",
            name: "Repo graph bootstrap",
            objective: "Index functions, symbols, tests, call edges, dataflow hints, ownership, and blast radius.",
            depends_on: &["source_of_truth"],
        },
        StageTemplate {
            slug: "contracts_and_slices",
            name: "Contracts and slices",
            objective: "Produce task contracts, proof commands, fixture strategy, and independent slices.",
            depends_on: &["architecture_blueprint", "repo_graph_bootstrap"],
        },
        StageTemplate {
            slug: "parallel_subsystems",
            name: "Parallel subsystems",
            objective: "Implement independent verified slices in isolated worktrees with critic lanes.",
            depends_on: &["contracts_and_slices"],
        },
        StageTemplate {
            slug: "integration_fusion",
            name: "Integration fusion",
            objective: "Fuse verified subsystem work, resolve interface drift, and run integration proof lanes.",
            depends_on: &["parallel_subsystems"],
        },
        StageTemplate {
            slug: "parity_lab",
            name: "Parity lab",
            objective: "Create differential, golden, metamorphic, and fuzz parity harnesses.",
            depends_on: &["source_of_truth", "contracts_and_slices"],
        },
        StageTemplate {
            slug: "parity_gap_closure",
            name: "Parity gap closure",
            objective: "Close blocking parity gaps using the gap ledger and regression proofs.",
            depends_on: &["integration_fusion", "parity_lab"],
        },
        StageTemplate {
            slug: "performance_closure",
            name: "Performance closure",
            objective: "Benchmark reference versus candidate and close hot-path gaps without weakening parity.",
            depends_on: &["parity_gap_closure"],
        },
        StageTemplate {
            slug: "hardening_security",
            name: "Hardening and security",
            objective: "Run fuzzing, stress, recovery, race, security, and fault-injection proof lanes.",
            depends_on: &["parity_gap_closure"],
        },
        StageTemplate {
            slug: "docs_release_ops",
            name: "Docs, release, and operations",
            objective: "Produce docs, CI gates, migration notes, release checklist, and operational runbooks.",
            depends_on: &["performance_closure", "hardening_security"],
        },
        StageTemplate {
            slug: "final_signoff",
            name: "Final signoff",
            objective: "Aggregate receipts, rerun full proofs, ensure clean tree, and require final approval.",
            depends_on: &["docs_release_ops"],
        },
    ]
}

fn stage_id_for(idx: usize, slug: &str) -> String {
    format!("stage-{:02}-{}", idx + 1, slug)
}

fn write_scope_for_stage(slug: &str) -> Vec<String> {
    match slug {
        "source_of_truth" | "architecture_blueprint" => vec![
            "docs/**".to_string(),
            "target/zyal/**".to_string(),
            ".jankurai/**".to_string(),
        ],
        "repo_graph_bootstrap" => vec![
            "target/zyal/repo-graph/**".to_string(),
            ".jankurai/**".to_string(),
        ],
        "parity_lab" | "parity_gap_closure" => vec![
            "tests/parity/**".to_string(),
            "target/zyal/parity/**".to_string(),
            "crates/**/tests/**".to_string(),
        ],
        "performance_closure" => vec![
            "benches/**".to_string(),
            "target/zyal/parity/**".to_string(),
            "target/zyal/perf/**".to_string(),
        ],
        "final_signoff" | "docs_release_ops" => vec![
            "target/zyal/**".to_string(),
            "docs/**".to_string(),
            ".jankurai/**".to_string(),
        ],
        _ => vec![
            "src/**".to_string(),
            "crates/**".to_string(),
            "tests/**".to_string(),
            "target/zyal/**".to_string(),
        ],
    }
}

/// Build a generic 9-12 stage super-reasoning master plan from a target
/// request.
///
/// The names mirror `jekko_runtime::daemon::super_reasoning::canonical_phases()`
/// so a runtime kicked off from this plan finds the phase ids it expects.
pub fn draft_super_master_plan(target: &PortTargetRequest) -> PortMasterPlan {
    draft_super_master_plan_with_config(target, &SuperReasoningConfig::default())
}

/// Like [`draft_super_master_plan`] but lets the caller override the
/// macro-stage target (clamped to 9..=12).
pub fn draft_super_master_plan_with_config(
    target: &PortTargetRequest,
    config: &SuperReasoningConfig,
) -> PortMasterPlan {
    let stage_count = config.effective_stage_target();
    let templates = canonical_stage_templates();
    let target_name = target.target.clone();
    let replacement = target.replacement.clone();

    // Slug -> stage_id table for dependency rewriting.
    let mut id_table = BTreeMap::new();
    for (idx, template) in templates.iter().take(stage_count).enumerate() {
        id_table.insert(template.slug, stage_id_for(idx, template.slug));
    }

    let mut stages = Vec::with_capacity(stage_count);
    let mut tasks = Vec::with_capacity(stage_count * 2);
    for (idx, template) in templates.iter().take(stage_count).enumerate() {
        let stage_id = id_table[template.slug].clone();
        let dependencies: Vec<String> = template
            .depends_on
            .iter()
            .filter_map(|slug| id_table.get(slug).cloned())
            .collect();
        let write_scope = write_scope_for_stage(template.slug);

        stages.push(PortStage {
            id: stage_id.clone(),
            ordinal: idx + 1,
            name: template.name.to_string(),
            objective: format!(
                "{} for {} -> {}. {}",
                template.name, target_name, replacement, template.objective
            ),
            status: if idx == 0 {
                PhaseStatus::Drafting
            } else {
                PhaseStatus::Planned
            },
            dependencies: dependencies.clone(),
            parallel_group: Some(format!("group-{:02}", idx + 1)),
            write_scope: write_scope.clone(),
            proof_lanes: vec!["just zyal-port-fast".to_string()],
            signoff_evidence: vec![
                "proof_receipt".to_string(),
                "replay_receipt".to_string(),
                "parity_receipt".to_string(),
            ],
        });

        let exec_task_id = format!("task-{:02}-{}-execute", idx + 1, template.slug);
        let signoff_task_id = format!("task-{:02}-{}-signoff", idx + 1, template.slug);
        tasks.push(PortMasterTask {
            id: exec_task_id.clone(),
            stage_id: stage_id.clone(),
            title: format!("Execute {} for {}", template.name, replacement),
            task_kind: "implementation".to_string(),
            risk_level: "medium".to_string(),
            write_scope: write_scope.clone(),
            bounded_write_scope: true,
            dependencies: Vec::new(),
            proof_lane: "just zyal-port-fast".to_string(),
            done_evidence: vec!["tests_passed".to_string(), "replay_receipt".to_string()],
            memory_scope: "run".to_string(),
            generated_zone_boundary_checks: true,
            status: MasterTaskStatus::Queued,
        });
        tasks.push(PortMasterTask {
            id: signoff_task_id,
            stage_id,
            title: format!(
                "Sign off {} with reducer, verifier, Jankurai, and parity receipts",
                template.name
            ),
            task_kind: "signoff".to_string(),
            risk_level: "medium".to_string(),
            write_scope: vec![
                "target/zyal/**".to_string(),
                ".jankurai/**".to_string(),
                "tests/parity/**".to_string(),
            ],
            bounded_write_scope: true,
            dependencies: vec![exec_task_id],
            proof_lane: "just zyal-port-fast".to_string(),
            done_evidence: vec![
                "jankurai_gate_passed".to_string(),
                "parity_receipt".to_string(),
            ],
            memory_scope: "run".to_string(),
            generated_zone_boundary_checks: true,
            status: MasterTaskStatus::Queued,
        });
    }

    PortMasterPlan {
        target: target.clone(),
        stages,
        tasks,
    }
}

/// Validate the macro-plan shape before persisting or fusing work.
///
/// Checks: stage count in `[SUPER_STAGE_MIN, SUPER_STAGE_MAX]`, unique stage
/// ids, and acyclic stage-dependency graph.
pub fn validate_super_macro_plan(plan: &PortMasterPlan) -> Result<()> {
    let count = plan.stages.len();
    if !(SUPER_STAGE_MIN..=SUPER_STAGE_MAX).contains(&count) {
        return Err(anyhow!(
            "super reasoning macro plan must contain {SUPER_STAGE_MIN}-{SUPER_STAGE_MAX} stages, got {count}"
        ));
    }
    let mut ids = BTreeSet::new();
    for stage in &plan.stages {
        if !ids.insert(stage.id.as_str()) {
            return Err(anyhow!("duplicate macro-stage id {}", stage.id));
        }
    }
    for stage in &plan.stages {
        for dep in &stage.dependencies {
            if !ids.contains(dep.as_str()) {
                return Err(anyhow!(
                    "macro-stage {} has unknown dependency {}",
                    stage.id,
                    dep
                ));
            }
        }
    }
    ensure_acyclic_stages(plan)?;
    Ok(())
}

fn ensure_acyclic_stages(plan: &PortMasterPlan) -> Result<()> {
    let graph: BTreeMap<&str, Vec<&str>> = plan
        .stages
        .iter()
        .map(|stage| {
            (
                stage.id.as_str(),
                stage.dependencies.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in graph.keys().copied() {
        visit(node, &graph, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit<'a>(
    node: &'a str,
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
) -> Result<()> {
    if visited.contains(node) {
        return Ok(());
    }
    if !visiting.insert(node) {
        return Err(anyhow!(
            "super reasoning macro plan has a stage dependency cycle through {node}"
        ));
    }
    for dep in graph.get(node).into_iter().flatten().copied() {
        if graph.contains_key(dep) {
            visit(dep, graph, visiting, visited)?;
        }
    }
    visiting.remove(node);
    visited.insert(node);
    Ok(())
}
