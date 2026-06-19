//! Compile-time ZYAL library resolution for `uses:` and first-class blocks.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};

use super::emit::strip_pragmas;

pub(super) const PAPER_BUILDER_REF: &str = "zyal://global/paper-builder@1";

#[derive(Debug, Clone, Default)]
pub(super) struct ResolvedUses {
    entries: Vec<ResolvedUse>,
}

impl ResolvedUses {
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn for_ref(&self, ref_id: &str) -> Option<&ResolvedUse> {
        self.entries.iter().find(|entry| entry.ref_id == ref_id)
    }

    pub(super) fn for_alias(&self, alias: &str) -> Option<&ResolvedUse> {
        self.entries.iter().find(|entry| entry.alias == alias)
    }

    pub(super) fn metadata_value(&self) -> Value {
        Value::Array(
            self.entries
                .iter()
                .map(|entry| {
                    json!({
                        "as": entry.alias,
                        "ref": entry.ref_id,
                        "source_hash": entry.source_hash,
                        "interface_hash": entry.interface_hash,
                    })
                })
                .collect(),
        )
    }
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedUse {
    pub(super) alias: String,
    pub(super) ref_id: String,
    pub(super) source_hash: String,
    pub(super) interface_hash: String,
}

#[derive(Debug, Clone)]
struct UseRequest {
    ref_id: String,
    alias: String,
}

/// Resolve all top-level `uses:` entries plus implicit first-class block refs.
pub(super) fn resolve(source: &Path, raw: &str) -> Result<ResolvedUses> {
    let body = strip_pragmas(raw);
    let root = parse_body(&body)?;
    let requests = requested_refs(&root)?;
    if requests.is_empty() {
        return Ok(ResolvedUses::default());
    }

    let mut by_alias: BTreeMap<String, ResolvedUse> = BTreeMap::new();
    for req in requests {
        if let Some(existing) = by_alias.get(&req.alias) {
            if existing.ref_id != req.ref_id {
                return Err(anyhow!(
                    "ZYAL_E_USE_ALIAS_CONFLICT: alias `{}` maps to both `{}` and `{}`",
                    req.alias,
                    existing.ref_id,
                    req.ref_id
                ));
            }
            continue;
        }
        let mut stack = Vec::new();
        let resolved = resolve_one(source, &req, &mut stack)?;
        by_alias.insert(req.alias, resolved);
    }
    Ok(ResolvedUses {
        entries: by_alias.into_values().collect(),
    })
}

fn resolve_one(source: &Path, req: &UseRequest, stack: &mut Vec<String>) -> Result<ResolvedUse> {
    if stack.iter().any(|seen| seen == &req.ref_id) {
        let mut cycle = stack.clone();
        cycle.push(req.ref_id.clone());
        return Err(anyhow!("ZYAL_E_REF_CYCLE: {}", cycle.join(" -> ")));
    }
    stack.push(req.ref_id.clone());
    let path = resolve_path(source, &req.ref_id)?;
    let raw = fs::read_to_string(&path).with_context(|| {
        format!(
            "ZYAL_E_MISSING_REF: read `{}` for `{}`",
            path.display(),
            req.ref_id
        )
    })?;
    let body = strip_pragmas(&raw);
    let root = parse_body(&body)?;
    let export = export_for_ref(&root, &req.ref_id).ok_or_else(|| {
        anyhow!(
            "ZYAL_E_MISSING_REF: `{}` does not export `{}`",
            path.display(),
            req.ref_id
        )
    })?;

    for child in requested_refs(&root)? {
        resolve_one(&path, &child, stack)?;
    }
    stack.pop();

    let interface = export
        .get("interface")
        .cloned()
        .unwrap_or_else(|| export.clone());
    let interface_json = serde_json::to_string(&canonicalize(interface))
        .context("serialize resolved ZYAL interface")?;
    Ok(ResolvedUse {
        alias: req.alias.clone(),
        ref_id: req.ref_id.clone(),
        source_hash: format!("sha256:{}", super::target::sha256(&raw)),
        interface_hash: format!("sha256:{}", super::target::sha256(&interface_json)),
    })
}

fn parse_body(body: &str) -> Result<Map<String, Value>> {
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(body).context("parse ZYAL body for uses resolution")?;
    let json_value = serde_json::to_value(parsed).context("convert ZYAL YAML to JSON")?;
    json_value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("ZYAL_E_USES_SHAPE: ZYAL body must be a mapping"))
}

fn requested_refs(root: &Map<String, Value>) -> Result<Vec<UseRequest>> {
    let mut requests = Vec::new();
    if let Some(uses) = root.get("uses") {
        let uses = uses
            .as_array()
            .ok_or_else(|| anyhow!("ZYAL_E_USES_SHAPE: `uses` must be a sequence"))?;
        for item in uses {
            let item = item
                .as_object()
                .ok_or_else(|| anyhow!("ZYAL_E_USES_SHAPE: each `uses` entry must be a mapping"))?;
            let ref_id = item
                .get("ref")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("ZYAL_E_USES_SHAPE: `uses[].ref` is required"))?;
            let alias = item
                .get("as")
                .and_then(Value::as_str)
                .unwrap_or_else(|| alias_from_ref(ref_id));
            requests.push(UseRequest {
                ref_id: ref_id.to_string(),
                alias: alias.to_string(),
            });
        }
    }
    if let Some(paper_builder) = root.get("paper_builder").and_then(Value::as_object) {
        let ref_id = paper_builder
            .get("use")
            .and_then(Value::as_str)
            .unwrap_or(PAPER_BUILDER_REF);
        if !requests
            .iter()
            .any(|req| req.ref_id == ref_id && req.alias == "paper_builder")
        {
            requests.push(UseRequest {
                ref_id: ref_id.to_string(),
                alias: "paper_builder".to_string(),
            });
        }
    }
    Ok(requests)
}

fn alias_from_ref(ref_id: &str) -> &str {
    ref_id
        .trim_start_matches("zyal://")
        .rsplit('/')
        .next()
        .and_then(|tail| tail.split('@').next())
        .filter(|tail| !tail.is_empty())
        .unwrap_or("library")
}

fn export_for_ref(root: &Map<String, Value>, ref_id: &str) -> Option<Value> {
    root.get("exports")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_object)
        .find(|export| export.get("ref").and_then(Value::as_str) == Some(ref_id))
        .map(|export| Value::Object(export.clone()))
}

fn resolve_path(source: &Path, ref_id: &str) -> Result<PathBuf> {
    if let Some(rest) = ref_id.strip_prefix("zyal://global/") {
        let name = ref_name_path(rest)?;
        let source_abs = absolute_source(source);
        let root = repo_root_for(&source_abs);
        let mut candidates = vec![root.join("agent/zyal/global").join(format!("{name}.zyal"))];
        if let Some(parent) = root.parent() {
            candidates.push(
                parent
                    .join("jekko-zyal")
                    .join("agent/zyal/global")
                    .join(format!("{name}.zyal")),
            );
        }
        for candidate in candidates {
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        return Err(anyhow!(
            "ZYAL_E_MISSING_REF: global ref `{ref_id}` was not found under `agent/zyal/global`"
        ));
    }
    if let Some(rest) = ref_id.strip_prefix("zyal://local/") {
        let rel = ref_name_path(rest)?;
        let mut path = absolute_source(source)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(rel);
        if path.extension().is_none() {
            path.set_extension("zyal");
        }
        if path.exists() {
            return Ok(path);
        }
        return Err(anyhow!(
            "ZYAL_E_MISSING_REF: local ref `{ref_id}` was not found"
        ));
    }
    Err(anyhow!(
        "ZYAL_E_UNSUPPORTED_REF: `{ref_id}` must start with `zyal://global/` or `zyal://local/`"
    ))
}

fn ref_name_path(rest: &str) -> Result<String> {
    let name = rest.split('@').next().unwrap_or(rest).trim_matches('/');
    if name.is_empty() || name.contains("..") || name.starts_with('/') {
        return Err(anyhow!("ZYAL_E_UNSUPPORTED_REF: invalid ref path `{rest}`"));
    }
    Ok(name.to_string())
}

fn absolute_source(source: &Path) -> PathBuf {
    if source.is_absolute() {
        return source.to_path_buf();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(source)
}

fn repo_root_for(source: &Path) -> PathBuf {
    for ancestor in source.ancestors() {
        if ancestor.join("agent/zyal/global").is_dir() {
            return ancestor.to_path_buf();
        }
    }
    for ancestor in source.ancestors() {
        if ancestor.join("Cargo.toml").exists() || ancestor.join(".git").exists() {
            return ancestor.to_path_buf();
        }
    }
    source
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> =
                map.into_iter().map(|(k, v)| (k, canonicalize(v))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        other => other,
    }
}
