use std::path::Path;

use anyhow::{anyhow, Context, Result};

use super::emit::strip_pragmas;

pub(super) fn validate_daemon_profile(source: &Path, raw: &str) -> Result<()> {
    if !raw.contains("<<<ZYAL v1:daemon") {
        return Err(anyhow!(
            "daemon profile missing daemon sentinel in {}",
            source.display()
        ));
    }
    if !raw.contains("<<<END_ZYAL") {
        return Err(anyhow!(
            "daemon profile missing END_ZYAL sentinel in {}",
            source.display()
        ));
    }
    Ok(())
}

pub(super) fn validate_runbook_profile(source: &Path, raw: &str) -> Result<()> {
    if !raw.contains("<<<ZYAL v1:") {
        return Err(anyhow!(
            "runbook profile missing ZYAL sentinel in {}",
            source.display()
        ));
    }
    if !raw.contains("<<<END_ZYAL") {
        return Err(anyhow!(
            "runbook profile missing END_ZYAL sentinel in {}",
            source.display()
        ));
    }
    Ok(())
}

/// Validate a SuperWorkflow `.zyal` source by parsing the YAML body and
/// running [`validate_superworkflow_value`] on it.
pub(super) fn validate_superworkflow_profile(source: &Path, raw: &str) -> Result<()> {
    let body = strip_pragmas(raw);
    let parsed: serde_yaml::Value = serde_yaml::from_str(&body)
        .with_context(|| format!("parse superworkflow YAML body in {}", source.display()))?;
    validate_superworkflow_value(source, &parsed)
}

/// Validate a parsed SuperWorkflow manifest value:
///
/// * `version`, `intent`, `confirm`, and `id` must be present with the
///   canonical SuperWorkflow literals.
/// * `job` must be a mapping; `job.name` and `job.objective` are required.
/// * `workflow` (or legacy `superworkflow`) must be a mapping either at the
///   root or nested under `job` for detector-compatible daemon envelopes.
/// * `workflow.phases` must be a sequence of 9-12 mappings.
/// * Each phase must have a unique non-empty `id`, an `objective`, and an
///   `exit` block with non-empty `required_artifacts` and `gates`.
/// * No phase may declare itself as a dependency, and every dependency must
///   reference a known phase id.
/// * The dependency graph must be acyclic (Kahn topological sort).
pub(super) fn validate_superworkflow_value(source: &Path, value: &serde_yaml::Value) -> Result<()> {
    let root = value.as_mapping().ok_or_else(|| {
        anyhow!(
            "superworkflow body in {} must be a YAML mapping",
            source.display()
        )
    })?;
    require_scalar(root, "version", "v1")?;
    require_scalar(root, "intent", "daemon")?;
    require_scalar(root, "confirm", "RUN_FOREVER")?;
    require_present(root, "id")?;

    let job = require_map(root, "job")?;
    require_present(job, "name")?;
    require_present(job, "objective")?;

    let workflow = lookup(root, "workflow");
    let workflow_is_state_machine = workflow
        .and_then(|value| value.as_mapping())
        .is_some_and(is_state_machine_workflow);
    if workflow_is_state_machine {
        validate_workflow_state_machine(source, require_map(root, "workflow")?)?;
    }
    let sw = if workflow_is_state_machine {
        match (
            lookup(root, "superworkflow"),
            lookup(job, "workflow"),
            lookup(job, "superworkflow"),
        ) {
            (Some(_), _, _) => require_map(root, "superworkflow")?,
            (None, Some(_), _) => require_map(job, "workflow")?,
            (None, None, Some(_)) => require_map(job, "superworkflow")?,
            _ => return Err(anyhow!("missing required key `superworkflow`")),
        }
    } else {
        match (
            lookup(root, "workflow"),
            lookup(root, "superworkflow"),
            lookup(job, "workflow"),
            lookup(job, "superworkflow"),
        ) {
            (Some(_), _, _, _) => require_map(root, "workflow")?,
            (None, Some(_), _, _) => require_map(root, "superworkflow")?,
            (None, None, Some(_), _) => require_map(job, "workflow")?,
            (None, None, None, Some(_)) => require_map(job, "superworkflow")?,
            _ => return Err(anyhow!("missing required key `workflow`")),
        }
    };
    let phases = require_seq(sw, "phases")?;
    if !(9..=12).contains(&phases.len()) {
        return Err(anyhow!(
            "superworkflow in {} requires 9-12 phases, got {}",
            source.display(),
            phases.len()
        ));
    }
    if let Some(stage_count) = lookup(sw, "stage_count") {
        if stage_count.as_u64().map(|v| v as usize) != Some(phases.len()) {
            return Err(anyhow!(
                "superworkflow stage_count must equal phases length in {}",
                source.display()
            ));
        }
    }

    let mut ids = std::collections::BTreeSet::new();
    for phase in phases {
        let phase = phase
            .as_mapping()
            .ok_or_else(|| anyhow!("superworkflow phase must be a mapping"))?;
        let id = lookup(phase, "id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("superworkflow phase missing id"))?;
        if id.trim().is_empty() {
            return Err(anyhow!("superworkflow phase id must be non-empty"));
        }
        if !ids.insert(id.to_string()) {
            return Err(anyhow!("duplicate superworkflow phase id {id}"));
        }
        require_present(phase, "objective")?;
        let exit = require_map(phase, "exit")?;
        if require_seq(exit, "required_artifacts")?.is_empty() {
            return Err(anyhow!("phase {id} must declare required_artifacts"));
        }
        if require_seq(exit, "gates")?.is_empty() {
            return Err(anyhow!("phase {id} must declare exit gates"));
        }
    }

    for phase in phases {
        let phase = phase.as_mapping().expect("checked above");
        let id = lookup(phase, "id")
            .and_then(|v| v.as_str())
            .expect("checked above");
        if let Some(depends_on) = lookup(phase, "depends_on") {
            let deps = depends_on
                .as_sequence()
                .ok_or_else(|| anyhow!("phase dependency list must be a sequence"))?;
            for dep in deps {
                let dep = dep
                    .as_str()
                    .ok_or_else(|| anyhow!("phase {id} dependency must be string"))?;
                if dep == id {
                    return Err(anyhow!("phase {id} depends on itself"));
                }
                if !ids.contains(dep) {
                    return Err(anyhow!("phase {id} depends on unknown phase {dep}"));
                }
            }
        }
    }
    if !superworkflow_dependencies_are_acyclic(phases)? {
        return Err(anyhow!(
            "superworkflow phase dependency graph contains a cycle"
        ));
    }
    Ok(())
}

fn superworkflow_dependencies_are_acyclic(phases: &[serde_yaml::Value]) -> Result<bool> {
    use std::collections::{BTreeMap, VecDeque};

    let mut indegree: BTreeMap<String, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for phase in phases {
        let phase = phase
            .as_mapping()
            .ok_or_else(|| anyhow!("superworkflow phase must be a mapping"))?;
        let id = lookup(phase, "id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("superworkflow phase missing id"))?
            .to_string();
        indegree.entry(id.clone()).or_insert(0);
        if let Some(depends_on) = lookup(phase, "depends_on") {
            // Dedupe before counting — see zyal-supervisor planner for the
            // same fix rationale (false-cycle on duplicate dep entries).
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            let deps = depends_on
                .as_sequence()
                .ok_or_else(|| anyhow!("phase dependency list must be a sequence"))?;
            for dep in deps {
                let dep = dep
                    .as_str()
                    .ok_or_else(|| anyhow!("phase dependency must be string"))?
                    .to_string();
                if !seen.insert(dep.clone()) {
                    continue;
                }
                outgoing.entry(dep).or_default().push(id.clone());
                *indegree.entry(id.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut ready: VecDeque<String> = indegree
        .iter()
        .filter_map(|(id, degree)| if *degree == 0 { Some(id.clone()) } else { None })
        .collect();
    let mut visited = 0usize;
    while let Some(id) = ready.pop_front() {
        visited += 1;
        if let Some(children) = outgoing.get(&id) {
            for next in children {
                let degree = indegree.get_mut(next).expect("known node");
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(next.clone());
                }
            }
        }
    }
    Ok(visited == indegree.len())
}

fn lookup<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    map.get(serde_yaml::Value::String(key.to_string()))
}

fn require_present<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Result<&'a serde_yaml::Value> {
    match lookup(map, key) {
        Some(value) => Ok(value),
        None => Err(anyhow!("missing required key `{key}`")),
    }
}

fn require_map<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Result<&'a serde_yaml::Mapping> {
    require_present(map, key)?
        .as_mapping()
        .ok_or_else(|| anyhow!("`{key}` must be a mapping"))
}

fn require_seq<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Result<&'a Vec<serde_yaml::Value>> {
    require_present(map, key)?
        .as_sequence()
        .ok_or_else(|| anyhow!("`{key}` must be a sequence"))
}

fn require_scalar(map: &serde_yaml::Mapping, key: &str, expected: &str) -> Result<()> {
    match require_present(map, key)?.as_str() {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(anyhow!("`{key}` must be `{expected}`")),
    }
}

fn is_state_machine_workflow(workflow: &serde_yaml::Mapping) -> bool {
    matches!(
        lookup(workflow, "type").and_then(|v| v.as_str()),
        Some("state_machine")
    ) && lookup(workflow, "initial").is_some()
        && lookup(workflow, "states").is_some()
}

fn validate_workflow_state_machine(source: &Path, workflow: &serde_yaml::Mapping) -> Result<()> {
    require_scalar(workflow, "type", "state_machine")?;
    let initial = require_present(workflow, "initial")?
        .as_str()
        .ok_or_else(|| anyhow!("workflow.initial must be a string"))?;
    let states = require_map(workflow, "states")?;
    if !(9..=12).contains(&states.len()) {
        return Err(anyhow!(
            "workflow in {} requires 9-12 states, got {}",
            source.display(),
            states.len()
        ));
    }
    if !states.contains_key(serde_yaml::Value::String(initial.to_string())) {
        return Err(anyhow!(
            "workflow.initial `{initial}` must name a declared state in {}",
            source.display()
        ));
    }

    let mut ids = std::collections::BTreeSet::new();
    let mut outgoing: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut indegree: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (state_name, state_value) in states {
        let state_id = state_name
            .as_str()
            .ok_or_else(|| anyhow!("workflow state keys must be strings"))?;
        if !ids.insert(state_id.to_string()) {
            return Err(anyhow!("duplicate workflow state id {state_id}"));
        }
        let state = state_value
            .as_mapping()
            .ok_or_else(|| anyhow!("workflow state `{state_id}` must be a mapping"))?;
        if let Some(writes) = lookup(state, "writes").and_then(|v| v.as_str()) {
            match writes {
                "scratch_only" | "isolated_worktree" | "main_worktree" => {}
                other => {
                    return Err(anyhow!(
                        "workflow state `{state_id}` has unsupported writes value `{other}`"
                    ));
                }
            }
        }
        if let Some(requires) = lookup(state, "requires") {
            requires.as_sequence().ok_or_else(|| {
                anyhow!("workflow state `{state_id}` requires must be a sequence")
            })?;
        }
        if let Some(produces) = lookup(state, "produces") {
            produces.as_sequence().ok_or_else(|| {
                anyhow!("workflow state `{state_id}` produces must be a sequence")
            })?;
        }
        indegree.entry(state_id.to_string()).or_insert(0);
        if let Some(transitions) = lookup(state, "transitions") {
            let transitions = transitions.as_sequence().ok_or_else(|| {
                anyhow!("workflow state `{state_id}` transitions must be a sequence")
            })?;
            let mut seen_targets = std::collections::BTreeSet::new();
            for transition in transitions {
                let transition = transition.as_mapping().ok_or_else(|| {
                    anyhow!("workflow transition in `{state_id}` must be a mapping")
                })?;
                let to = lookup(transition, "to")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("workflow transition in `{state_id}` missing `to`"))?;
                if to == state_id {
                    return Err(anyhow!("workflow state `{state_id}` depends on itself"));
                }
                if seen_targets.insert(to.to_string()) {
                    outgoing
                        .entry(state_id.to_string())
                        .or_default()
                        .push(to.to_string());
                    *indegree.entry(to.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    for (state_id, targets) in &outgoing {
        for target in targets {
            if !ids.contains(target) {
                return Err(anyhow!(
                    "workflow state `{state_id}` transitions to unknown state `{target}`"
                ));
            }
        }
    }

    let mut ready: std::collections::VecDeque<String> = indegree
        .iter()
        .filter_map(|(id, degree)| if *degree == 0 { Some(id.clone()) } else { None })
        .collect();
    let mut visited = 0usize;
    while let Some(id) = ready.pop_front() {
        visited += 1;
        if let Some(children) = outgoing.get(&id) {
            for next in children {
                let degree = indegree.get_mut(next).expect("known workflow state");
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(next.clone());
                }
            }
        }
    }
    if visited != ids.len() {
        return Err(anyhow!(
            "workflow state machine contains a cycle in {}",
            source.display()
        ));
    }

    Ok(())
}
