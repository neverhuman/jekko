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
/// * `job` and `superworkflow` must be mappings; `job.name` and
///   `job.objective` are required.
/// * `superworkflow.phases` must be a sequence of 9-12 mappings.
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

    let sw = require_map(root, "superworkflow")?;
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
            for dep in depends_on.as_sequence().cloned().unwrap_or_default() {
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
            for dep in depends_on.as_sequence().cloned().unwrap_or_default() {
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
        for next in outgoing.get(&id).cloned().unwrap_or_default() {
            let degree = indegree.get_mut(&next).expect("known node");
            *degree -= 1;
            if *degree == 0 {
                ready.push_back(next);
            }
        }
    }
    Ok(visited == indegree.len())
}

fn lookup<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    map.get(serde_yaml::Value::String(key.to_string()))
}

fn require_present<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Result<&'a serde_yaml::Value> {
    lookup(map, key).ok_or_else(|| anyhow!("missing required key `{key}`"))
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
    let actual = require_present(map, key)?.as_str().unwrap_or_default();
    if actual == expected {
        Ok(())
    } else {
        Err(anyhow!("`{key}` must be `{expected}`"))
    }
}
