use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::profile::Profile;

pub(super) fn default_target(source: &Path, profile: &Profile) -> PathBuf {
    match profile {
        Profile::DeclarativeToml { .. } => {
            // Declarative `.zyal` sources canonically live under `agent/zyal/`
            // per the jankurai v1.0.0 conformance rule; the compiled TOML
            // belongs one directory up at `agent/<stem>.toml` so other tools
            // (proof-lanes, validators) find it on the well-known path.
            let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
            if let Some(parent) = source.parent().and_then(|p| p.parent()) {
                parent.join(format!("{stem}.toml"))
            } else {
                source.with_extension("toml")
            }
        }
        Profile::Workflow { .. } => {
            let stem = source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("workflow");
            PathBuf::from(format!(".github/workflows/{stem}.yml"))
        }
        Profile::Runbook => source.with_extension("yml"),
        Profile::Daemon { .. } => source.to_path_buf(),
        Profile::SuperWorkflow { .. } => {
            // SuperWorkflow `.zyal` sources live under `agent/zyal/`; the
            // emitted canonical JSON sits at
            // `agent/superworkflows/<stem>.superworkflow.json` so downstream
            // supervisors discover it on a well-known path.
            let stem = source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("workflow");
            if let Some(parent) = source.parent().and_then(|p| p.parent()) {
                parent
                    .join("superworkflows")
                    .join(format!("{stem}.superworkflow.json"))
            } else {
                source.with_extension("superworkflow.json")
            }
        }
    }
}

pub(super) fn source_reference(source: &Path) -> String {
    source
        .strip_prefix(".")
        .unwrap_or(source)
        .display()
        .to_string()
}

pub(super) fn sha256(raw: &str) -> String {
    let mut h = Sha256::new();
    h.update(raw.as_bytes());
    format!("{:x}", h.finalize())
}
