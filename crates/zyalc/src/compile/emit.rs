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
    let stamped = stamp_generated_header(json_value);
    let rendered = serde_json::to_string_pretty(&stamped).context("render SuperWorkflow JSON")?;
    Ok(format!("{rendered}\n"))
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
    raw.lines()
        .filter(|line| !line.trim_start().starts_with("# zyal:"))
        .filter(|line| !line.trim_start().starts_with("# zyalc:"))
        .collect::<Vec<_>>()
        .join("\n")
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
