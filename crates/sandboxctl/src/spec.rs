//! TOML schema for `generated/sandbox-lanes.toml` + validation.
//!
//! Source-of-truth for both the Rust runtime (`sandboxctl`) and the TypeScript
//! schema mirror in `packages/jekko/src/config/sandbox-lanes.ts`. Drift
//! between the two surfaces is caught by `tests/spec_schema.rs` + the TS
//! schema test under `packages/jekko/test/agent/`.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

pub use crate::spec_types::*;

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse TOML at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("validation: {0}")]
    Validation(String),
}

pub fn load(path: &Path) -> Result<LanesDoc, SpecError> {
    let bytes = fs::read_to_string(path).map_err(|source| SpecError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&bytes, path)
}

pub fn parse(bytes: &str, path: &Path) -> Result<LanesDoc, SpecError> {
    let doc: LanesDoc = toml::from_str(bytes).map_err(|source| SpecError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    validate(&doc)?;
    Ok(doc)
}

pub fn validate(doc: &LanesDoc) -> Result<(), SpecError> {
    if doc.schema_version.is_empty() {
        return Err(SpecError::Validation("schema_version is empty".into()));
    }
    if doc.lanes.is_empty() {
        return Err(SpecError::Validation("no lanes defined".into()));
    }

    let mut seen_names: HashSet<&str> = HashSet::new();
    let mut seen_ids: HashSet<&str> = HashSet::new();

    for lane in &doc.lanes {
        check_uniqueness(lane, &mut seen_names, &mut seen_ids)?;
        check_command_patterns(lane)?;
        check_timeouts_and_image(lane)?;
        check_path_interpolations(lane)?;
    }
    Ok(())
}

fn check_uniqueness<'a>(
    lane: &'a Lane,
    seen_names: &mut HashSet<&'a str>,
    seen_ids: &mut HashSet<&'a str>,
) -> Result<(), SpecError> {
    if !seen_names.insert(&lane.name) {
        return Err(SpecError::Validation(format!(
            "duplicate lane name: {}",
            lane.name
        )));
    }
    if !seen_ids.insert(&lane.command_id) {
        return Err(SpecError::Validation(format!(
            "duplicate command_id: {}",
            lane.command_id
        )));
    }
    Ok(())
}

fn check_command_patterns(lane: &Lane) -> Result<(), SpecError> {
    if lane.commands.allowed_patterns.is_empty() {
        return Err(SpecError::Validation(format!(
            "lane {}: commands.allowed_patterns must not be empty (whitelist semantics)",
            lane.name
        )));
    }
    for pattern in lane
        .commands
        .allowed_patterns
        .iter()
        .chain(lane.commands.denied_patterns.iter())
    {
        if pattern.trim().is_empty() {
            return Err(SpecError::Validation(format!(
                "lane {}: empty pattern not allowed",
                lane.name
            )));
        }
        if pattern == "*" {
            return Err(SpecError::Validation(format!(
                "lane {}: bare `*` pattern would defeat the allowlist; use specific globs",
                lane.name
            )));
        }
    }
    Ok(())
}

fn check_timeouts_and_image(lane: &Lane) -> Result<(), SpecError> {
    if lane.timeout_seconds == 0 {
        return Err(SpecError::Validation(format!(
            "lane {}: timeout_seconds must be > 0",
            lane.name
        )));
    }
    if matches!(lane.runtime.backend, Backend::Docker | Backend::Podman)
        && lane.runtime.image.is_none()
    {
        return Err(SpecError::Validation(format!(
            "lane {}: runtime.image is required for docker/podman backends",
            lane.name
        )));
    }
    if lane.runtime.timeout_seconds == 0 {
        return Err(SpecError::Validation(format!(
            "lane {}: runtime.timeout_seconds must be > 0",
            lane.name
        )));
    }
    Ok(())
}

fn check_path_interpolations(lane: &Lane) -> Result<(), SpecError> {
    if !lane.export.patch_path.contains("{run_id}") {
        return Err(SpecError::Validation(format!(
            "lane {}: export.patch_path must include the {{run_id}} interpolation",
            lane.name
        )));
    }
    for path in [
        &lane.environment.home,
        &lane.environment.tmpdir,
        &lane.environment.cache_home,
    ] {
        if !path.contains("{run_id}") && !path.starts_with('/') {
            return Err(SpecError::Validation(format!(
                "lane {}: environment path '{}' must be absolute or include the {{run_id}} interpolation",
                lane.name, path
            )));
        }
    }
    Ok(())
}
