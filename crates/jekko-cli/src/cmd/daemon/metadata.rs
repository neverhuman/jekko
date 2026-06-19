use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub(super) fn metadata_path() -> Result<PathBuf> {
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is required for daemon metadata")?
        .join(".jekko");
    Ok(base.join("jekko-daemon.json"))
}

pub(super) fn write_metadata(metadata: &DaemonMetadata) -> Result<()> {
    let path = metadata_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_string_pretty(metadata)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub(super) fn read_metadata() -> Result<DaemonMetadata> {
    let path = metadata_path()?;
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

pub(super) fn db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("JEKKO_DB") {
        return path.into();
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".jekko").join("jekko.db"),
        None => PathBuf::from("jekko.db"),
    }
}

pub(super) fn resolve_runner_bin() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("JANKURAI_RUNNER_BIN") {
        return Ok(path.into());
    }
    let current = std::env::current_exe().context("resolve current executable")?;
    if let Some(parent) = current.parent() {
        let sibling = parent.join("jekko-runner");
        if sibling.exists() {
            return Ok(sibling);
        }
    }
    Ok(PathBuf::from("jekko-runner"))
}

pub(super) fn last_line(path: &std::path::Path) -> Option<String> {
    let text = fs::read_to_string(path).unwrap_or_default();
    text.lines().last().map(str::to_string)
}

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn random_run_id() -> String {
    format!("port-{}", now_secs())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DaemonMetadata {
    pub(super) pid: u32,
    pub(super) kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) repo: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) port_config: Option<PathBuf>,
    pub(super) started_at: u64,
}

impl DaemonMetadata {
    pub(super) fn event_log_path(&self) -> Option<PathBuf> {
        Some(
            self.repo
                .as_ref()?
                .join("target/zyal/runs")
                .join(self.run_id.as_ref()?)
                .join("events.jsonl"),
        )
    }
}
