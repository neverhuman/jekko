//! Manifest parsing helpers for `jekko port-run`.
//!
//! Split out of `port_run.rs` to keep that file under the 500-LOC shape
//! threshold (jankurai HLT-001:shape). All callers live in the parent
//! `port_run` module — public surface is `pub(super)` only.

use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value as JsonValue;
use tempfile::NamedTempFile;
use zyal_supervisor::{Phase, SuperWorkflow, SupervisorStore};

use super::PortRunArgs;

/// Resolve the supervisor DB path: `--db` if set, else
/// `~/.jekko/zyal-supervisor.sqlite`.
pub(super) fn resolve_db_path(args: &PortRunArgs) -> Result<PathBuf> {
    if let Some(p) = args.db.as_ref() {
        return Ok(p.clone());
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| anyhow!("HOME is not set; pass --db <PATH> explicitly"))?;
    let mut path = PathBuf::from(home);
    path.push(".jekko");
    path.push("zyal-supervisor.sqlite");
    Ok(path)
}

pub(super) fn open_store(args: &PortRunArgs, in_memory: bool) -> Result<SupervisorStore> {
    if in_memory {
        return SupervisorStore::open_in_memory().context("open in-memory supervisor store");
    }
    let path = resolve_db_path(args)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    SupervisorStore::open(&path)
        .with_context(|| format!("open supervisor store at {}", path.display()))
}

/// Load a SuperWorkflow manifest from `path`. `.zyal` files are compiled
/// via the `zyalc` binary into a tempfile; `.json` files are read directly.
pub(super) fn load_manifest(path: &Path) -> Result<SuperWorkflow> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let json_text = match ext.as_str() {
        "json" => std::fs::read_to_string(path)
            .with_context(|| format!("read manifest {}", path.display()))?,
        "zyal" => compile_zyal_to_json(path)?,
        other => bail!(
            "unsupported manifest extension `.{other}`; expected `.zyal` or `.json` (at {})",
            path.display()
        ),
    };
    parse_supervisor_manifest(&json_text, path)
}

/// Shell out to `cargo run -p zyalc --offline -- compile <path> --out <tempfile>`
/// and return the JSON text. Tempfile is created with `tempfile::NamedTempFile`
/// so it cleans up after a successful read.
fn compile_zyal_to_json(path: &Path) -> Result<String> {
    let tmp = NamedTempFile::new().context("allocate tempfile for zyalc output")?;
    let tmp_path = tmp.path().to_path_buf();
    let status = ProcCommand::new("cargo")
        .args([
            "run",
            "-p",
            "zyalc",
            "--offline",
            "--quiet",
            "--",
            "compile",
        ])
        .arg(path)
        .arg("--out")
        .arg(&tmp_path)
        .status()
        .with_context(|| format!("invoke zyalc to compile {}", path.display()))?;
    if !status.success() {
        bail!(
            "zyalc failed to compile {} (exit code {:?})",
            path.display(),
            status.code()
        );
    }
    let text = std::fs::read_to_string(&tmp_path)
        .with_context(|| format!("read compiled manifest at {}", tmp_path.display()))?;
    Ok(text)
}

/// Parse the JSON text produced by zyalc into the supervisor's
/// `SuperWorkflow` shape. The two formats differ slightly — zyalc emits a
/// nested `{id, job:{name,objective,...}, superworkflow:{phases:[...]}}`
/// document, while the supervisor model is flat. We adapt here so callers
/// can hand any of the three accepted shapes:
///
/// 1. The flat supervisor shape (used directly by tests + simple manifests).
/// 2. The nested zyalc emission shape (`agent/superworkflows/*.json`).
/// 3. A bare `{phases: [...]}` document carrying just the DAG.
fn parse_supervisor_manifest(text: &str, source: &Path) -> Result<SuperWorkflow> {
    let value: JsonValue = serde_json::from_str(text)
        .with_context(|| format!("parse manifest JSON at {}", source.display()))?;
    // Prefer the flat shape when both `phases` and `objective` are present at
    // the top level (the supervisor's canonical encoding).
    if value.get("phases").is_some() && value.get("objective").is_some() {
        return serde_json::from_value::<SuperWorkflow>(value)
            .with_context(|| format!("decode flat SuperWorkflow at {}", source.display()));
    }
    // Otherwise treat the document as the zyalc-nested emission shape.
    adapt_zyalc_emission(&value, source)
}

/// Translate the `agent/superworkflows/*.json` shape into a flat
/// `SuperWorkflow`. Best-effort: anything outside the supervisor's core
/// fields is dropped silently for the scaffold; richer policies are a
/// follow-up.
fn adapt_zyalc_emission(value: &JsonValue, source: &Path) -> Result<SuperWorkflow> {
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("manifest at {} is missing `id`", source.display()))?
        .to_string();
    let job = value.get("job");
    let name = job
        .and_then(|j| j.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(id.as_str())
        .to_string();
    let objective = job
        .and_then(|j| j.get("objective"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Two shapes accepted: zyalc's nested emission (`{superworkflow: {phases:
    // [...]}}`) and the flat supervisor shape (`{phases: [...]}`). Explicit
    // matches keep the typed-state contract clean (no `or_else` fallback
    // soup); unknown shapes hit the third arm with a precise error.
    let phases_node =
        if let Some(nested) = value.get("superworkflow").and_then(|sw| sw.get("phases")) {
            nested
        } else if let Some(flat) = value.get("phases") {
            flat
        } else {
            return Err(anyhow!(
                "manifest at {} is missing `superworkflow.phases` (or top-level `phases`)",
                source.display()
            ));
        };
    let raw_phases: Vec<JsonValue> = match phases_node.as_array() {
        Some(array) => array.clone(),
        None => {
            return Err(anyhow!(
                "`phases` at {} must be a JSON array",
                source.display()
            ))
        }
    };
    let mut phases: Vec<Phase> = Vec::with_capacity(raw_phases.len());
    for (idx, raw) in raw_phases.into_iter().enumerate() {
        let phase_id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("phase #{idx} at {} is missing `id`", source.display()))?
            .to_string();
        let phase_name = raw
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(phase_id.as_str())
            .to_string();
        let phase_objective = raw
            .get("objective")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let depends_on: Vec<String> = match raw.get("depends_on").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect(),
            None => Vec::new(),
        };
        phases.push(Phase {
            id: phase_id,
            name: phase_name,
            objective: phase_objective,
            depends_on,
            write_scope: Default::default(),
            signoff: Default::default(),
            gates: Vec::new(),
        });
    }
    Ok(SuperWorkflow {
        id,
        name,
        objective,
        phases,
        controller: Default::default(),
        memory: Default::default(),
        sandbox: Default::default(),
        repo_graph: Default::default(),
        parity: Default::default(),
    })
}
