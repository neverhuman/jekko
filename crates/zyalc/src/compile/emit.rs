use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::profile::Profile;

#[path = "emit_toml.rs"]
mod emit_toml_impl;
pub(crate) use emit_toml_impl::render as emit_toml;

/// Returns (body, comment-line-prefix). The comment prefix differs between
/// TOML (`# `) and YAML (`# `), so we centralise it here for the trailer.
pub(super) fn emit(
    profile: &Profile,
    raw: &str,
    source: Option<&Path>,
) -> Result<(String, String)> {
    match profile {
        Profile::Runbook => Ok((raw.to_string(), String::new())),
        Profile::DeclarativeToml { .. } => Ok((emit_toml(raw)?, "# ".into())),
        Profile::Workflow { .. } => Ok((emit_workflow(raw)?, "# ".into())),
        Profile::Daemon { .. } => Err(anyhow!("daemon profiles are validation-only")),
        // SuperWorkflow emits canonical JSON; JSON has no comment syntax so
        // the banner is suppressed in `compile_one` and the header prefix is
        // empty here.
        Profile::SuperWorkflow { .. } => Ok((emit_superworkflow(raw)?, String::new())),
        // FlowGraph likewise emits canonical JSON (the diagram IR).
        Profile::FlowGraph { .. } => Ok((emit_flowgraph_with_source(raw, source)?, String::new())),
    }
}

/// Emit a FlowGraph manifest as the canonical flattened agent-flow JSON IR.
///
/// Validation is re-run against the parsed YAML so a direct caller of
/// `emit_flowgraph` (notably the unit tests) cannot bypass the structural
/// checks performed by [`super::validation::validate_flowgraph_profile`].
pub(super) fn emit_flowgraph(raw: &str) -> Result<String> {
    emit_flowgraph_with_source(raw, None)
}

pub(super) fn emit_flowgraph_with_source(raw: &str, source: Option<&Path>) -> Result<String> {
    let body = strip_pragmas(raw);
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&body).context("parse flowgraph YAML body")?;
    super::validation::validate_flowgraph_value(std::path::Path::new("<memory>"), &parsed)?;
    let uses = match source {
        Some(source) => super::uses::resolve(source, raw)?,
        None => super::uses::ResolvedUses::default(),
    };
    let ir = super::flowgraph::build_flowgraph(&parsed, &body, &uses)?;
    let rendered = serde_json::to_string_pretty(&ir).context("render FlowGraph JSON")?;
    Ok(format!("{rendered}\n"))
}

fn emit_workflow(raw: &str) -> Result<String> {
    let body = strip_pragmas(raw);
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&body).context("parse workflow YAML body")?;
    let rendered = serde_yaml::to_string(&parsed).context("render workflow YAML")?;
    Ok(rendered)
}

/// Emit a SuperWorkflow manifest as canonical JSON.
///
/// Validation is re-run against the parsed YAML so a direct caller of
/// `emit_superworkflow` (notably the unit tests) cannot bypass the structural
/// checks performed by [`super::validation::validate_superworkflow_profile`].
pub(super) fn emit_superworkflow(raw: &str) -> Result<String> {
    let body = strip_pragmas(raw);
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&body).context("parse superworkflow YAML body")?;
    super::validation::validate_superworkflow_value(std::path::Path::new("<memory>"), &parsed)?;
    // Convert YAML → JSON Value so we can prepend the generated-header
    // object (jankurai's HLT-002-GENERATED-MUTATION rule expects every
    // generated zone file to declare its source + regeneration command;
    // pure JSON has no comment syntax so we surface that header as a
    // top-level `_generated` object instead).
    let json_value = serde_json::to_value(&parsed).context("YAML → JSON for SuperWorkflow")?;
    let json_value = normalize_superworkflow_shape(json_value);
    let stamped = stamp_generated_header(json_value);
    let rendered = serde_json::to_string_pretty(&stamped).context("render SuperWorkflow JSON")?;
    Ok(format!("{rendered}\n"))
}

fn normalize_superworkflow_shape(mut value: serde_json::Value) -> serde_json::Value {
    let Some(root) = value.as_object_mut() else {
        return value;
    };
    if root.contains_key("superworkflow") {
        return value;
    }
    if let Some(generated) = superworkflow_from_state_machine(root) {
        root.insert("superworkflow".to_string(), generated);
        return value;
    }
    if let Some(workflow) = root.remove("workflow") {
        root.insert("superworkflow".to_string(), workflow);
        return value;
    }
    let superworkflow = {
        let Some(job) = root
            .get_mut("job")
            .and_then(serde_json::Value::as_object_mut)
        else {
            return value;
        };
        job.remove("workflow")
            .or_else(|| job.remove("superworkflow"))
    };
    if let Some(superworkflow) = superworkflow {
        root.insert("superworkflow".to_string(), superworkflow);
    }
    value
}

fn superworkflow_from_state_machine(
    root: &serde_json::Map<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    use serde_json::{Map, Value};
    let workflow = root.get("workflow")?.as_object()?;
    if workflow.get("type").and_then(Value::as_str) != Some("state_machine") {
        return None;
    }
    let states = workflow.get("states")?.as_object()?;
    let mut outgoing: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    for (state_id, state_value) in states {
        let Some(transitions) = state_value
            .as_object()
            .and_then(|state| state.get("transitions"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        let mut seen = std::collections::BTreeSet::new();
        for transition in transitions {
            let Some(to) = transition
                .as_object()
                .and_then(|transition| transition.get("to"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if !seen.insert(to.to_string()) {
                continue;
            }
            outgoing
                .entry(state_id.clone())
                .or_default()
                .push(to.to_string());
        }
    }

    let job = root.get("job")?.as_object()?;
    let id = root.get("id")?.as_str()?.to_string();
    let name = job
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(id.as_str())
        .to_string();
    let objective = job
        .get("objective")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut phases = Vec::with_capacity(states.len());
    for (state_id, state_value) in states {
        let state = state_value.as_object()?;
        let mut deps = Vec::new();
        for (candidate, children) in &outgoing {
            if children.iter().any(|child| child == state_id) {
                deps.push(candidate.clone());
            }
        }
        deps.sort();
        deps.dedup();
        let writes = match state.get("writes").and_then(Value::as_str) {
            Some("scratch_only") => "scratch_only",
            Some("main_worktree") => "primary_repo",
            Some("isolated_worktree") => "isolated_worktree",
            _ => "isolated_worktree",
        };
        let signoff = if state.get("approval").is_some() || state.get("terminal").is_some() {
            "single"
        } else {
            "none"
        };
        phases.push(Value::Object(Map::from_iter([
            ("id".to_string(), Value::String(state_id.clone())),
            (
                "name".to_string(),
                Value::String(state_id.replace('_', " ")),
            ),
            ("objective".to_string(), Value::String(state_id.clone())),
            (
                "depends_on".to_string(),
                Value::Array(deps.into_iter().map(Value::String).collect()),
            ),
            ("write_scope".to_string(), Value::String(writes.to_string())),
            ("signoff".to_string(), Value::String(signoff.to_string())),
            ("gates".to_string(), Value::Array(Vec::new())),
        ])));
    }

    Some(Value::Object(Map::from_iter([
        ("id".to_string(), Value::String(id)),
        ("name".to_string(), Value::String(name)),
        ("objective".to_string(), Value::String(objective)),
        ("phases".to_string(), Value::Array(phases)),
    ])))
}

/// Prepend a `_generated` top-level object to the rendered JSON so the
/// generated-zone audit (`HLT-002-GENERATED-MUTATION`) can detect that the
/// file is a tool output rather than hand-authored. Preserves all other
/// keys in their original serde-defined order.
fn stamp_generated_header(value: serde_json::Value) -> serde_json::Value {
    use serde_json::{Map, Value};
    let stamp = serde_json::json!({
        "by": "zyalc",
        "schema": "zyal/superworkflow@1",
        "do_not_edit_by_hand": true,
        "regenerate": "cargo run -p zyalc -- compile <source.zyal>",
    });
    match value {
        Value::Object(orig) => {
            let mut out = Map::with_capacity(orig.len() + 1);
            out.insert("_generated".to_string(), stamp);
            for (k, v) in orig {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        // Non-object root: leave untouched (the validator would have
        // rejected this shape upstream, so we shouldn't see it here).
        other => other,
    }
}

pub(super) fn strip_pragmas(raw: &str) -> String {
    strip_pragma_lines(&strip_zyal_envelope(raw))
}

fn toml_source_body(raw: &str) -> String {
    let before_end = strip_pragma_lines(&strip_zyal_envelope(raw));
    if let Some(after_end) = body_after_zyal_envelope(raw) {
        let after_end = strip_pragma_lines(&after_end);
        if !after_end.trim().is_empty() {
            if before_end.trim().is_empty() {
                return after_end;
            }
            return format!("{before_end}\n{after_end}");
        }
    }
    strip_pragmas(raw)
}

fn strip_pragma_lines(body: &str) -> String {
    body.lines()
        .filter(|line| !line.trim_start().starts_with("# zyal:"))
        .filter(|line| !line.trim_start().starts_with("# zyalc:"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_zyal_envelope(raw: &str) -> String {
    let mut in_body = false;
    let mut body = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if !in_body {
            if trimmed.starts_with("<<<ZYAL ") {
                in_body = true;
            }
            continue;
        }
        if trimmed.starts_with("<<<END_ZYAL ") {
            return body.join("\n");
        }
        body.push(line);
    }
    raw.to_string()
}

fn body_after_zyal_envelope(raw: &str) -> Option<String> {
    let mut after_end = false;
    let mut body = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if !after_end {
            if trimmed.starts_with("<<<END_ZYAL ") {
                after_end = true;
            }
            continue;
        }
        if trimmed.starts_with("ZYAL_ARM ") {
            break;
        }
        body.push(line);
    }
    if after_end {
        Some(body.join("\n"))
    } else {
        None
    }
}
