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
        return Ok(());
    }
    let sw = match (
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

/// Validate a FlowGraph `.zyal` source by parsing the YAML body and running
/// [`validate_flowgraph_value`] on it.
pub(super) fn validate_flowgraph_profile(source: &Path, raw: &str) -> Result<()> {
    let body = strip_pragmas(raw);
    let parsed: serde_yaml::Value = serde_yaml::from_str(&body)
        .with_context(|| format!("parse flowgraph YAML body in {}", source.display()))?;
    validate_flowgraph_value(source, &parsed)
}

/// Validate a parsed FlowGraph manifest value.
///
/// FlowGraph validation is intentionally LENIENT — the IR is meant to be robust
/// and AI-authorable, fully mapping arbitrary agent flows:
///
/// * `id`, `job.name`, and `job.objective` are required (the framing).
/// * When an explicit `graph.nodes` block is present, each node must have a
///   unique non-empty `id` and a `type`.
/// * `graph.edges` (when present) must each carry a `from`/`src` and `to`/`dst`.
///
/// Deliberately NOT enforced: acyclicity (`loop_back` edges express iteration),
/// and edge-endpoint membership (edges may reference runtime-resolved or
/// implicit nodes that the compiler expands).
pub(super) fn validate_flowgraph_value(source: &Path, value: &serde_yaml::Value) -> Result<()> {
    let root = value.as_mapping().ok_or_else(|| {
        anyhow!(
            "flowgraph body in {} must be a YAML mapping",
            source.display()
        )
    })?;
    // M4-cont: validate the optional `zyal:` contract header before anything
    // else, so a document that targets an unsupported dialect / registry / IR
    // fails fast with a precise diagnostic.
    validate_flowgraph_header(root)?;
    validate_uses_shape(root)?;
    validate_paper_builder_block(root)?;
    require_present(root, "id")?;
    let job = require_map(root, "job")?;
    require_present(job, "name")?;
    require_present(job, "objective")?;

    let Some(graph) = lookup(root, "graph").and_then(|v| v.as_mapping()) else {
        return Ok(()); // no authored graph — synthesized from policy blocks
    };
    if let Some(nodes) = lookup(graph, "nodes").and_then(|v| v.as_sequence()) {
        let mut ids = std::collections::BTreeSet::new();
        for node in nodes {
            let node = node
                .as_mapping()
                .ok_or_else(|| anyhow!("flowgraph graph.node must be a mapping"))?;
            let id = lookup(node, "id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("flowgraph graph.node missing id"))?;
            if id.trim().is_empty() {
                return Err(anyhow!("flowgraph graph.node id must be non-empty"));
            }
            // `.` is reserved as the `node.port` edge separator (F3); a dotted id
            // would make edge-endpoint resolution ambiguous.
            if id.contains('.') {
                return Err(anyhow!(
                    "flowgraph graph.node id `{id}` must not contain `.` (reserved for node.port edge refs)"
                ));
            }
            if !ids.insert(id.to_string()) {
                return Err(anyhow!("duplicate flowgraph graph.node id {id}"));
            }
            require_present(node, "type")?;
            // F2: an authored node must name a registered kind. Synthesized
            // graphs (no `graph.nodes`) never reach here and only emit known
            // kinds. Adding a kind is a `node-types.json` entry, not a code edit.
            let ty = lookup(node, "type").and_then(|v| v.as_str()).unwrap_or("");
            if !ty.is_empty() && !super::registry::known_node_kind(ty) {
                return Err(anyhow!(
                    "ZYAL_E_UNKNOWN_NODE_KIND: flowgraph graph.node `{id}` has unknown type `{ty}` (register it in contracts/node-types.json)"
                ));
            }
        }
    }
    if let Some(edges) = lookup(graph, "edges").and_then(|v| v.as_sequence()) {
        for edge in edges {
            let edge = edge
                .as_mapping()
                .ok_or_else(|| anyhow!("flowgraph graph.edge must be a mapping"))?;
            let from = lookup(edge, "from").or_else(|| lookup(edge, "src"));
            let to = lookup(edge, "to").or_else(|| lookup(edge, "dst"));
            if from.and_then(|v| v.as_str()).is_none() {
                return Err(anyhow!("flowgraph graph.edge missing `from`/`src`"));
            }
            if to.and_then(|v| v.as_str()).is_none() {
                return Err(anyhow!("flowgraph graph.edge missing `to`/`dst`"));
            }
        }
    }
    Ok(())
}

fn validate_uses_shape(root: &serde_yaml::Mapping) -> Result<()> {
    let mut aliases = std::collections::BTreeMap::<String, String>::new();
    let Some(uses) = lookup(root, "uses") else {
        return Ok(());
    };
    let uses = uses
        .as_sequence()
        .ok_or_else(|| anyhow!("ZYAL_E_USES_SHAPE: `uses` must be a sequence"))?;
    for item in uses {
        let item = item
            .as_mapping()
            .ok_or_else(|| anyhow!("ZYAL_E_USES_SHAPE: each `uses` entry must be a mapping"))?;
        let ref_id = lookup(item, "ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("ZYAL_E_USES_SHAPE: `uses[].ref` is required"))?;
        if !ref_id.starts_with("zyal://") {
            return Err(anyhow!(
                "ZYAL_E_UNSUPPORTED_REF: `uses[].ref` `{ref_id}` must be a zyal:// ref"
            ));
        }
        let mut alias_value = None;
        if let Some(alias) = lookup(item, "as") {
            let alias = alias
                .as_str()
                .ok_or_else(|| anyhow!("ZYAL_E_USES_SHAPE: `uses[].as` must be a string"))?;
            if alias.trim().is_empty() {
                return Err(anyhow!("ZYAL_E_USES_SHAPE: `uses[].as` must be non-empty"));
            }
            alias_value = Some(alias.to_string());
        }
        if let Some(alias) = alias_value {
            if let Some(existing) = aliases.insert(alias.clone(), ref_id.to_string()) {
                if existing != ref_id {
                    return Err(anyhow!(
                        "ZYAL_E_USE_ALIAS_CONFLICT: alias `{alias}` maps to both `{existing}` and `{ref_id}`"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_paper_builder_block(root: &serde_yaml::Mapping) -> Result<()> {
    let Some(block) = lookup(root, "paper_builder") else {
        return Ok(());
    };
    let block = block
        .as_mapping()
        .ok_or_else(|| anyhow!("paper_builder must be a mapping"))?;
    let id = lookup(block, "id")
        .and_then(|v| v.as_str())
        .unwrap_or("paper_builder");
    if id.trim().is_empty() || id.contains('.') {
        return Err(anyhow!(
            "paper_builder.id must be non-empty and must not contain `.`"
        ));
    }
    if let Some(use_ref) = lookup(block, "use").and_then(|v| v.as_str()) {
        if !use_ref.starts_with("zyal://") {
            return Err(anyhow!(
                "ZYAL_E_UNSUPPORTED_REF: paper_builder.use `{use_ref}` must be a zyal:// ref"
            ));
        }
    }
    let goal = lookup(block, "paper_goal")
        .or_else(|| lookup(block, "goal"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if goal.trim().is_empty() {
        return Err(anyhow!("paper_builder.paper_goal is required"));
    }
    for key in ["data_artifacts", "success_criteria", "authors"] {
        let seq = lookup(block, key)
            .ok_or_else(|| anyhow!("paper_builder.{key} is required"))?
            .as_sequence()
            .ok_or_else(|| anyhow!("paper_builder.{key} must be a sequence"))?;
        if seq.is_empty() {
            return Err(anyhow!("paper_builder.{key} must not be empty"));
        }
    }
    if let Some(mode) = lookup(block, "mode").and_then(|v| v.as_str()) {
        if !matches!(mode, "light" | "medium" | "heavy") {
            return Err(anyhow!(
                "paper_builder.mode must be one of light, medium, heavy"
            ));
        }
    }
    if let Some(target) = lookup(block, "journal_target").and_then(|v| v.as_str()) {
        if target != "ieee" {
            return Err(anyhow!(
                "paper_builder.journal_target currently supports ieee"
            ));
        }
    }
    for author in lookup(block, "authors")
        .and_then(|v| v.as_sequence())
        .into_iter()
        .flatten()
    {
        let author = author
            .as_mapping()
            .ok_or_else(|| anyhow!("paper_builder.authors entries must be mappings"))?;
        for key in ["name", "affiliation", "email"] {
            let value = lookup(author, key)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("paper_builder.authors[].{key} is required"))?;
            if value.trim().is_empty() {
                return Err(anyhow!("paper_builder.authors[].{key} must be non-empty"));
            }
        }
    }
    Ok(())
}

/// Highest ZYAL dialect major this compiler accepts. v1/v2 documents compile;
/// a declared major above this is rejected (`ZYAL_E_UNSUPPORTED_MAJOR`).
const SUPPORTED_DIALECT_MAJOR: u64 = 2;
/// The IR envelope version this compiler emits (mirrors `flowgraph::IR_VERSION`).
/// A document requiring a newer IR is rejected (`ZYAL_E_IR_TOO_NEW_FOR_RUNNER`).
const SUPPORTED_IR_VERSION: u64 = 3;

/// Validate the optional top-level `zyal:` contract header.
///
/// Absent header → OK (the implicit v1 dialect, for back-compat with the 36
/// pre-header examples). When PRESENT, the header must be well-formed and must
/// not require a dialect / registry / IR newer than this compiler supports:
///   * no `version` under `zyal:` → `ZYAL_E_VERSION_MISSING`
///   * `version` major > supported → `ZYAL_E_UNSUPPORTED_MAJOR`
///   * `contract.registry_version` newer than the embedded registry →
///     `ZYAL_E_REGISTRY_TOO_OLD`
///   * `contract.ir_version` newer than the emitted IR →
///     `ZYAL_E_IR_TOO_NEW_FOR_RUNNER`
fn validate_flowgraph_header(root: &serde_yaml::Mapping) -> Result<()> {
    let Some(zyal) = lookup(root, "zyal").and_then(|v| v.as_mapping()) else {
        return Ok(());
    };
    let Some(version) = lookup(zyal, "version") else {
        return Err(anyhow!(
            "ZYAL_E_VERSION_MISSING: a `zyal:` header must declare a `version`"
        ));
    };
    let version = scalar_str(version);
    let major = parse_major(&version).ok_or_else(|| {
        anyhow!("ZYAL_E_VERSION_MISSING: `zyal.version` `{version}` is not a valid version")
    })?;
    if major > SUPPORTED_DIALECT_MAJOR {
        return Err(anyhow!(
            "ZYAL_E_UNSUPPORTED_MAJOR: document declares ZYAL dialect major {major}, but this compiler supports up to {SUPPORTED_DIALECT_MAJOR}"
        ));
    }

    if let Some(contract) = lookup(zyal, "contract").and_then(|v| v.as_mapping()) {
        if let Some(rv) = lookup(contract, "registry_version") {
            let rv = scalar_str(rv);
            let embedded = super::registry::registry_version();
            if semver_gt(&rv, embedded) {
                return Err(anyhow!(
                    "ZYAL_E_REGISTRY_TOO_OLD: document requires node registry {rv}, but this compiler embeds {embedded}"
                ));
            }
        }
        if let Some(ir) = lookup(contract, "ir_version") {
            let ir = scalar_str(ir);
            let required = parse_ir_version(&ir);
            if required > SUPPORTED_IR_VERSION {
                return Err(anyhow!(
                    "ZYAL_E_IR_TOO_NEW_FOR_RUNNER: document requires IR `{ir}` (v{required}), but this compiler emits flowgraph.v{SUPPORTED_IR_VERSION}"
                ));
            }
        }
    }
    Ok(())
}

/// Render a YAML scalar (string or number) as a plain string.
fn scalar_str(v: &serde_yaml::Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(n) = v.as_u64() {
        return n.to_string();
    }
    if let Some(f) = v.as_f64() {
        return f.to_string();
    }
    String::new()
}

/// First dotted component of a version, stripping an optional leading `v`.
fn parse_major(ver: &str) -> Option<u64> {
    let ver = ver.trim().trim_start_matches('v');
    ver.split('.').next()?.parse::<u64>().ok()
}

/// Parse `MAJOR.MINOR.PATCH` (missing components default to 0) for comparison.
fn semver_tuple(ver: &str) -> (u64, u64, u64) {
    let ver = ver.trim().trim_start_matches('v');
    let mut it = ver.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

fn semver_gt(a: &str, b: &str) -> bool {
    semver_tuple(a) > semver_tuple(b)
}

/// Extract the trailing integer of an IR version string like `flowgraph.v3`.
fn parse_ir_version(ir: &str) -> u64 {
    ir.rsplit('.')
        .next()
        .map(|tail| tail.trim_start_matches('v'))
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(0)
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
