//! Phase-DAG validation and readiness helpers.
//!
//! All functions in this module are pure — they take a [`SuperWorkflow`]
//! reference and return values. State is owned by [`crate::store`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use thiserror::Error;

use crate::model::SuperWorkflow;

/// Validation errors for SuperWorkflow manifests.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum ValidationError {
    /// The manifest contains zero phases.
    #[error("manifest has no phases")]
    EmptyManifest,
    /// Phase count outside the 9..=12 canonical range.
    #[error("superworkflow requires 9..=12 phases, got {0}")]
    PhaseCountOutOfRange(usize),
    /// Two phases share the same id.
    #[error("duplicate phase id `{0}`")]
    DuplicatePhaseId(String),
    /// A phase lists itself in `depends_on`.
    #[error("phase `{0}` depends on itself")]
    SelfDependency(String),
    /// A `depends_on` entry references a phase that does not exist.
    #[error("phase `{phase}` depends on unknown phase `{dep}`")]
    UnknownDependency {
        /// Phase declaring the dependency.
        phase: String,
        /// Missing dependency target.
        dep: String,
    },
    /// The dependency graph contains a cycle. The vector lists phase ids
    /// that remain unscheduled when the topological sort stalls.
    #[error("phase dependency graph contains a cycle: {0:?}")]
    CycleDetected(Vec<String>),
}

/// Validate a SuperWorkflow manifest with runtime-grade constraints.
///
/// Checks:
/// - non-empty
/// - 9..=12 phases
/// - unique phase ids
/// - no self-dependency
/// - every `depends_on` resolves
/// - acyclic
pub fn validate_manifest(manifest: &SuperWorkflow) -> Result<(), ValidationError> {
    let phases = &manifest.phases;
    if phases.is_empty() {
        return Err(ValidationError::EmptyManifest);
    }
    let len = phases.len();
    if !(9..=12).contains(&len) {
        return Err(ValidationError::PhaseCountOutOfRange(len));
    }

    let mut ids = BTreeSet::new();
    for phase in phases {
        if !ids.insert(phase.id.clone()) {
            return Err(ValidationError::DuplicatePhaseId(phase.id.clone()));
        }
    }

    for phase in phases {
        for dep in &phase.depends_on {
            if dep == &phase.id {
                return Err(ValidationError::SelfDependency(phase.id.clone()));
            }
            if !ids.contains(dep) {
                return Err(ValidationError::UnknownDependency {
                    phase: phase.id.clone(),
                    dep: dep.clone(),
                });
            }
        }
    }

    // execution_layers re-validates UnknownDependency but is responsible for
    // CycleDetected. Any other error here would be a programmer bug.
    execution_layers(manifest).map(|_| ())
}

/// Return topological execution layers ("waves") for the phase DAG.
///
/// Each inner vector contains phase ids whose dependencies are all in
/// strictly earlier layers; phases within a layer can run in parallel.
pub fn execution_layers(manifest: &SuperWorkflow) -> Result<Vec<Vec<String>>, ValidationError> {
    let phases = &manifest.phases;
    let mut indegree: BTreeMap<String, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let id_set: BTreeSet<&str> = phases.iter().map(|p| p.id.as_str()).collect();

    for phase in phases {
        indegree.entry(phase.id.clone()).or_insert(0);
        // Dedupe before counting so a phase declaring the same dep twice
        // (e.g. depends_on: ["p01", "p01"]) doesn't inflate the indegree
        // beyond what `outgoing` can ever decrement, which would falsely
        // strand the phase and look like a cycle.
        let unique_deps: BTreeSet<&String> = phase.depends_on.iter().collect();
        for dep in unique_deps {
            if !id_set.contains(dep.as_str()) {
                return Err(ValidationError::UnknownDependency {
                    phase: phase.id.clone(),
                    dep: dep.clone(),
                });
            }
            outgoing
                .entry(dep.clone())
                .or_default()
                .push(phase.id.clone());
            *indegree.entry(phase.id.clone()).or_insert(0) += 1;
        }
    }

    let mut ready: VecDeque<String> = indegree
        .iter()
        .filter_map(|(id, deg)| if *deg == 0 { Some(id.clone()) } else { None })
        .collect();
    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut visited = 0usize;

    while !ready.is_empty() {
        let mut layer: Vec<String> = Vec::with_capacity(ready.len());
        let wave = std::mem::take(&mut ready);
        for id in wave {
            visited += 1;
            layer.push(id.clone());
            if let Some(next_ids) = outgoing.get(&id).cloned() {
                for next in next_ids {
                    let deg = indegree
                        .get_mut(&next)
                        .expect("indegree initialized for every phase");
                    *deg -= 1;
                    if *deg == 0 {
                        ready.push_back(next);
                    }
                }
            }
        }
        // Stable ordering inside a layer makes test assertions deterministic.
        layer.sort();
        layers.push(layer);
    }

    if visited == phases.len() {
        Ok(layers)
    } else {
        let unscheduled: Vec<String> = indegree
            .into_iter()
            .filter_map(|(id, deg)| if deg > 0 { Some(id) } else { None })
            .collect();
        Err(ValidationError::CycleDetected(unscheduled))
    }
}

/// Return phases whose dependencies are all in `completed` and that are not
/// listed in `blocked`. Already-completed and already-blocked phases are
/// filtered out.
pub fn ready_phases(
    manifest: &SuperWorkflow,
    completed: &[String],
    blocked: &[String],
) -> Vec<String> {
    let completed_set: BTreeSet<&str> = completed.iter().map(String::as_str).collect();
    let blocked_set: BTreeSet<&str> = blocked.iter().map(String::as_str).collect();
    manifest
        .phases
        .iter()
        .filter(|p| !completed_set.contains(p.id.as_str()))
        .filter(|p| !blocked_set.contains(p.id.as_str()))
        .filter(|p| {
            p.depends_on
                .iter()
                .all(|dep| completed_set.contains(dep.as_str()))
        })
        .map(|p| p.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ControllerPolicy, Gate, GateKind, MemoryPolicy, ParityPolicy, Phase, PhaseSignoffMode,
        RepoGraphPolicy, SandboxPolicy, SuperWorkflow, WriteScope,
    };

    fn phase(id: &str, deps: &[&str]) -> Phase {
        Phase {
            id: id.into(),
            name: format!("Phase {id}"),
            objective: format!("Objective for {id}"),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            write_scope: WriteScope::IsolatedWorktree,
            signoff: PhaseSignoffMode::Single,
            gates: vec![Gate {
                name: "tests_green".into(),
                kind: GateKind::TestsGreen,
                required: true,
            }],
        }
    }

    fn workflow_with(phases: Vec<Phase>) -> SuperWorkflow {
        SuperWorkflow {
            id: "wf-test".into(),
            name: "Test Workflow".into(),
            objective: "Verify the planner".into(),
            phases,
            controller: ControllerPolicy::default(),
            memory: MemoryPolicy::default(),
            sandbox: SandboxPolicy::default(),
            repo_graph: RepoGraphPolicy::default(),
            parity: ParityPolicy::default(),
        }
    }

    fn linear_phases(count: usize) -> Vec<Phase> {
        (0..count)
            .map(|i| {
                let id = format!("p{i:02}");
                let deps: Vec<String> = if i == 0 {
                    vec![]
                } else {
                    vec![format!("p{:02}", i - 1)]
                };
                let deps_ref: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
                phase(&id, &deps_ref)
            })
            .collect()
    }

    #[test]
    fn validates_minimum_9_phases() {
        let wf = workflow_with(linear_phases(9));
        validate_manifest(&wf).expect("9 phases must validate");

        let too_few = workflow_with(linear_phases(8));
        assert_eq!(
            validate_manifest(&too_few),
            Err(ValidationError::PhaseCountOutOfRange(8)),
        );
    }

    #[test]
    fn validates_maximum_12_phases() {
        let wf = workflow_with(linear_phases(12));
        validate_manifest(&wf).expect("12 phases must validate");

        let too_many = workflow_with(linear_phases(13));
        assert_eq!(
            validate_manifest(&too_many),
            Err(ValidationError::PhaseCountOutOfRange(13)),
        );
    }

    #[test]
    fn detects_empty_manifest() {
        let wf = workflow_with(vec![]);
        assert_eq!(validate_manifest(&wf), Err(ValidationError::EmptyManifest));
    }

    #[test]
    fn detects_duplicate_phase_id() {
        let mut phases = linear_phases(9);
        phases[3].id = phases[2].id.clone();
        let wf = workflow_with(phases);
        match validate_manifest(&wf) {
            Err(ValidationError::DuplicatePhaseId(id)) => assert_eq!(id, "p02"),
            other => panic!("expected DuplicatePhaseId, got {other:?}"),
        }
    }

    #[test]
    fn detects_self_dependency() {
        let mut phases = linear_phases(9);
        let own_id = phases[4].id.clone();
        phases[4].depends_on.push(own_id);
        let wf = workflow_with(phases);
        match validate_manifest(&wf) {
            Err(ValidationError::SelfDependency(id)) => assert_eq!(id, "p04"),
            other => panic!("expected SelfDependency, got {other:?}"),
        }
    }

    #[test]
    fn detects_unknown_dependency() {
        let mut phases = linear_phases(9);
        phases[5].depends_on.push("p99".into());
        let wf = workflow_with(phases);
        match validate_manifest(&wf) {
            Err(ValidationError::UnknownDependency { phase, dep }) => {
                assert_eq!(phase, "p05");
                assert_eq!(dep, "p99");
            }
            other => panic!("expected UnknownDependency, got {other:?}"),
        }
    }

    #[test]
    fn detects_cycle() {
        // Build a 9-phase fan-out/in DAG, then close a back-edge to create a cycle.
        let phases = vec![
            phase("p00", &[]),
            phase("p01", &["p00"]),
            phase("p02", &["p00"]),
            phase("p03", &["p01", "p02"]),
            phase("p04", &["p01"]),
            phase("p05", &["p03", "p04"]),
            phase("p06", &["p03"]),
            phase("p07", &["p05", "p06"]),
            // back-edge: p08 depends on p07 (forward, fine) AND p00 depends on
            // p08 (back-edge, cycle). Inject after.
            phase("p08", &["p07"]),
        ];
        let mut wf = workflow_with(phases);
        wf.phases[0].depends_on.push("p08".into());
        match validate_manifest(&wf) {
            Err(ValidationError::CycleDetected(stuck)) => {
                assert!(!stuck.is_empty(), "cycle members must be reported");
            }
            other => panic!("expected CycleDetected, got {other:?}"),
        }
    }

    #[test]
    fn execution_layers_topological() {
        let phases = vec![
            phase("p00", &[]),
            phase("p01", &["p00"]),
            phase("p02", &["p00"]),
            phase("p03", &["p01", "p02"]),
            phase("p04", &["p01"]),
            phase("p05", &["p03", "p04"]),
            phase("p06", &["p03"]),
            phase("p07", &["p05", "p06"]),
            phase("p08", &["p07"]),
        ];
        let wf = workflow_with(phases);
        let layers = execution_layers(&wf).expect("DAG must yield layers");
        assert_eq!(layers[0], vec!["p00".to_string()]);
        assert_eq!(layers[1], vec!["p01".to_string(), "p02".to_string()]);
        assert!(layers[2].contains(&"p03".to_string()));
        assert!(layers[2].contains(&"p04".to_string()));
        // Last layer is the sink.
        assert_eq!(layers.last().unwrap(), &vec!["p08".to_string()]);
        // All 9 phases scheduled exactly once.
        let total: usize = layers.iter().map(Vec::len).sum();
        assert_eq!(total, 9);
    }

    #[test]
    fn ready_phases_handles_completion() {
        let phases = vec![
            phase("p00", &[]),
            phase("p01", &["p00"]),
            phase("p02", &["p00"]),
            phase("p03", &["p01", "p02"]),
            phase("p04", &["p01"]),
            phase("p05", &["p03", "p04"]),
            phase("p06", &["p03"]),
            phase("p07", &["p05", "p06"]),
            phase("p08", &["p07"]),
        ];
        let wf = workflow_with(phases);

        // Nothing completed, only the root is ready.
        let ready = ready_phases(&wf, &[], &[]);
        assert_eq!(ready, vec!["p00".to_string()]);

        // p00 done → p01, p02 ready.
        let completed = vec!["p00".to_string()];
        let mut ready = ready_phases(&wf, &completed, &[]);
        ready.sort();
        assert_eq!(ready, vec!["p01".to_string(), "p02".to_string()]);

        // p00 done + p01 blocked → only p02 ready.
        let blocked = vec!["p01".to_string()];
        let ready = ready_phases(&wf, &completed, &blocked);
        assert_eq!(ready, vec!["p02".to_string()]);

        // Already-completed phases are filtered out of ready set.
        let completed = vec!["p00".to_string(), "p01".to_string(), "p02".to_string()];
        let mut ready = ready_phases(&wf, &completed, &[]);
        ready.sort();
        assert!(ready.contains(&"p03".to_string()));
        assert!(ready.contains(&"p04".to_string()));
        assert!(!ready.contains(&"p00".to_string()));
    }
}
