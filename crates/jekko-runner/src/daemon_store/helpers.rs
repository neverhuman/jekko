use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use jekko_store::db::Db;
use sha1::{Digest, Sha1};

/// Resolve the writable Jekko SQLite database path.
pub fn default_db_path(repo_root: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("JEKKO_DB") {
        return path.into();
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".jekko").join("jekko.db");
    }
    repo_root.join("agent/zyal/jekko.db")
}

/// Open the durable Jekko DB, creating parent directories if needed.
pub fn open_db(repo_root: &Path) -> Result<Db> {
    let path = default_db_path(repo_root);
    open_db_at(&path)
}

/// Open a specific durable Jekko DB path.
pub fn open_db_at(path: &Path) -> Result<Db> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    Db::open(path).with_context(|| format!("open Jekko DB at {}", path.display()))
}

pub fn target_id(run_id: &str) -> String {
    format!("port-target-{run_id}")
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn project_id_for(repo_root: &Path) -> String {
    let mut hasher = Sha1::new();
    hasher.update(repo_root.display().to_string().as_bytes());
    format!("zyal-project-{:x}", hasher.finalize())[..26].to_string()
}

pub(crate) fn hash_json(value: &serde_json::Value) -> Result<String> {
    let mut hasher = Sha1::new();
    let bytes = serde_json::to_vec(value).context("serialize daemon run spec for hash")?;
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn label<T: serde::Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?.trim_matches('"').to_string())
}
