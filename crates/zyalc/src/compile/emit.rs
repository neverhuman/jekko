use anyhow::{anyhow, Context, Result};

use crate::profile::Profile;

/// Returns (body, comment-line-prefix). The comment prefix differs between
/// TOML (`# `) and YAML (`# `), so we centralise it here for the trailer.
pub(super) fn emit(profile: &Profile, raw: &str) -> Result<(String, String)> {
    match profile {
        Profile::Runbook => Ok((raw.to_string(), String::new())),
        Profile::DeclarativeToml { .. } => Ok((emit_toml(raw)?, "# ".into())),
        Profile::Workflow { .. } => Ok((emit_workflow(raw)?, "# ".into())),
        Profile::Daemon { .. } => Err(anyhow!("daemon profiles are validation-only")),
        // SuperWorkflow emits canonical JSON; JSON has no comment syntax so
        // the banner is suppressed in `compile_one` and the header prefix is
        // empty here.
        Profile::SuperWorkflow { .. } => Ok((emit_superworkflow(raw)?, String::new())),
    }
}

pub(super) fn emit_toml(raw: &str) -> Result<String> {
    let body = strip_pragmas(raw);
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&body).context("parse declarative YAML body")?;
    let toml_value = yaml_to_toml(parsed)?;
    let rendered = toml::to_string_pretty(&toml_value).context("render TOML")?;
    Ok(rendered)
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
    strip_zyal_envelope(raw)
        .lines()
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

/// Convert a YAML mapping into a TOML value. The declarative schema uses
/// `lanes: [...]` at the top level; TOML's natural array-of-tables form is
/// `[[lane]]`, so we rename the key during translation.
fn yaml_to_toml(value: serde_yaml::Value) -> Result<toml::Value> {
    use serde_yaml::Value as Y;
    let map = match value {
        Y::Mapping(m) => m,
        _ => return Err(anyhow!("declarative body must be a YAML mapping")),
    };
    let sandbox = match map.get(Y::String("sandbox".to_string())) {
        Some(Y::Mapping(sandbox)) => Some(sandbox.clone()),
        Some(_) => return Err(anyhow!("sandbox must be a mapping")),
        None => None,
    };
    let job = match map.get(Y::String("job".to_string())) {
        Some(Y::Mapping(job)) => Some(job.clone()),
        Some(_) => return Err(anyhow!("job must be a mapping")),
        None => None,
    };
    let has_lane_contract = map.contains_key(Y::String("lanes".to_string()))
        || map.contains_key(Y::String("schema_version".to_string()))
        || map.contains_key(Y::String("sandbox_root".to_string()))
        || job
            .as_ref()
            .is_some_and(|nested| nested.contains_key(Y::String("lanes".to_string())))
        || sandbox
            .as_ref()
            .is_some_and(|nested| nested.contains_key(Y::String("lanes".to_string())));
    if has_lane_contract {
        let mut tbl = toml::value::Table::new();
        let schema_version = match map.get(Y::String("schema_version".to_string())) {
            Some(value) => yaml_value_to_toml(value.clone())?,
            None => match sandbox
                .as_ref()
                .and_then(|nested| nested.get(Y::String("schema_version".to_string())))
            {
                Some(value) => yaml_value_to_toml(value.clone())?,
                None => return Err(anyhow!("schema_version is required")),
            },
        };
        tbl.insert("schema_version".into(), schema_version);
        if let Some(sandbox_root) = map.get(Y::String("sandbox_root".to_string())) {
            tbl.insert(
                "sandbox_root".into(),
                yaml_value_to_toml(sandbox_root.clone())?,
            );
        } else if let Some(sandbox_root) = sandbox
            .as_ref()
            .and_then(|nested| nested.get(Y::String("sandbox_root".to_string())))
        {
            tbl.insert(
                "sandbox_root".into(),
                yaml_value_to_toml(sandbox_root.clone())?,
            );
        }
        let lanes = match map.get(Y::String("lanes".to_string())) {
            Some(Y::Sequence(arr)) => arr.clone(),
            Some(_) => return Err(anyhow!("lanes must be a sequence")),
            None => match job
                .as_ref()
                .and_then(|nested| nested.get(Y::String("lanes".to_string())))
            {
                Some(Y::Sequence(arr)) => arr.clone(),
                Some(_) => return Err(anyhow!("lanes must be a sequence")),
                None => match sandbox
                    .as_ref()
                    .and_then(|nested| nested.get(Y::String("lanes".to_string())))
                {
                    Some(Y::Sequence(arr)) => arr.clone(),
                    Some(_) => return Err(anyhow!("lanes must be a sequence")),
                    None => return Err(anyhow!("lanes are required")),
                },
            },
        };
        let mut arr = Vec::with_capacity(lanes.len());
        for item in lanes {
            arr.push(yaml_value_to_toml(item)?);
        }
        tbl.insert("lane".into(), toml::Value::Array(arr));
        return Ok(toml::Value::Table(tbl));
    }
    let map = match sandbox {
        Some(sandbox) => sandbox,
        None => map,
    };
    let mut tbl = toml::value::Table::new();
    for (k, v) in map {
        let key = match k.as_str() {
            Some(s) => s.to_string(),
            None => return Err(anyhow!("non-string key")),
        };
        if key == "lanes" {
            let array = match v.as_sequence() {
                Some(arr) => arr.clone(),
                None => return Err(anyhow!("lanes must be a sequence")),
            };
            let mut arr = Vec::with_capacity(array.len());
            for item in array {
                arr.push(yaml_value_to_toml(item)?);
            }
            tbl.insert("lane".into(), toml::Value::Array(arr));
        } else {
            tbl.insert(key, yaml_value_to_toml(v)?);
        }
    }
    Ok(toml::Value::Table(tbl))
}

fn yaml_value_to_toml(v: serde_yaml::Value) -> Result<toml::Value> {
    use serde_yaml::Value as Y;
    Ok(match v {
        Y::Null => toml::Value::String(String::new()),
        Y::Bool(b) => toml::Value::Boolean(b),
        Y::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(u) = n.as_u64() {
                toml::Value::Integer(u as i64)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                toml::Value::String(n.to_string())
            }
        }
        Y::String(s) => toml::Value::String(s),
        Y::Sequence(seq) => {
            let mut arr = Vec::with_capacity(seq.len());
            for item in seq {
                arr.push(yaml_value_to_toml(item)?);
            }
            toml::Value::Array(arr)
        }
        Y::Mapping(m) => {
            let mut tbl = toml::value::Table::new();
            for (k, v) in m {
                let key = match k.as_str() {
                    Some(s) => s.to_string(),
                    None => return Err(anyhow!("non-string key in mapping")),
                };
                tbl.insert(key, yaml_value_to_toml(v)?);
            }
            toml::Value::Table(tbl)
        }
        Y::Tagged(t) => yaml_value_to_toml(t.value)?,
    })
}
