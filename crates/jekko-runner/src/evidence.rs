//! Bounded evidence loading for live-proof port runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::hashing::sha256_hex;
use crate::port::{EvidenceInput, EvidenceInputKind};

/// One loaded evidence source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedEvidence {
    /// Input id from config.
    pub id: String,
    /// Source kind.
    pub kind: EvidenceInputKind,
    /// Evidence role.
    pub role: String,
    /// Expanded source path or URL.
    pub source: String,
    /// Bytes loaded before UTF-8 clipping.
    pub bytes_read: usize,
    /// Whether content was clipped to the source max.
    pub clipped: bool,
    /// Stable content hash.
    pub sha256: String,
    /// Loaded content. This is bounded by `max_bytes`.
    pub content: String,
    /// Optional unavailable reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

impl LoadedEvidence {
    /// Storage-safe receipt without full content.
    pub fn receipt(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "kind": self.kind,
            "role": self.role,
            "source": self.source,
            "bytes_read": self.bytes_read,
            "clipped": self.clipped,
            "sha256": self.sha256,
            "unavailable_reason": self.unavailable_reason,
        })
    }
}

/// Load all configured evidence inputs. Local paths are resolved relative to
/// `repo_root`; URLs are disabled unless `ZYAL_ALLOW_URL_EVIDENCE=1`.
pub fn load_evidence_inputs(
    repo_root: &Path,
    inputs: &[EvidenceInput],
) -> Result<Vec<LoadedEvidence>> {
    let mut out = Vec::new();
    for input in inputs {
        match input.kind {
            EvidenceInputKind::File => {
                let path = resolve_path(repo_root, &input.path_or_url);
                out.push(load_file(input, &path)?);
            }
            EvidenceInputKind::Glob => {
                let paths = expand_simple_glob(repo_root, &input.path_or_url)?;
                for path in paths {
                    out.push(load_file(input, &path)?);
                }
            }
            EvidenceInputKind::Url => out.push(load_url(input)?),
        }
    }
    Ok(out)
}

fn load_file(input: &EvidenceInput, path: &Path) -> Result<LoadedEvidence> {
    let bytes = fs::read(path).with_context(|| format!("read evidence {}", path.display()))?;
    let (content, clipped) = clip_bytes(&bytes, input.max_bytes);
    Ok(LoadedEvidence {
        id: input.id.clone(),
        kind: input.kind,
        role: input.role.clone(),
        source: path.display().to_string(),
        bytes_read: bytes.len().min(input.max_bytes),
        clipped,
        sha256: sha256_hex(content.as_bytes()),
        content,
        unavailable_reason: None,
    })
}

fn load_url(input: &EvidenceInput) -> Result<LoadedEvidence> {
    if std::env::var("ZYAL_ALLOW_URL_EVIDENCE").ok().as_deref() != Some("1") {
        return Ok(LoadedEvidence {
            id: input.id.clone(),
            kind: input.kind,
            role: input.role.clone(),
            source: input.path_or_url.clone(),
            bytes_read: 0,
            clipped: false,
            sha256: sha256_hex(b""),
            content: String::new(),
            unavailable_reason: Some("url_evidence_disabled".to_string()),
        });
    }
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "10"])
        .arg(&input.path_or_url)
        .output()
        .with_context(|| format!("fetch evidence URL {}", input.path_or_url))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!(
            "fetch evidence URL {} failed: {}",
            input.path_or_url,
            error
        ));
    }
    let (content, clipped) = clip_bytes(&output.stdout, input.max_bytes);
    Ok(LoadedEvidence {
        id: input.id.clone(),
        kind: input.kind,
        role: input.role.clone(),
        source: input.path_or_url.clone(),
        bytes_read: output.stdout.len().min(input.max_bytes),
        clipped,
        sha256: sha256_hex(content.as_bytes()),
        content,
        unavailable_reason: None,
    })
}

fn resolve_path(repo_root: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    }
}

fn expand_simple_glob(repo_root: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let pattern = resolve_path(repo_root, pattern);
    let Some(pattern_text) = pattern.to_str() else {
        anyhow::bail!("glob path is not valid UTF-8: {}", pattern.display());
    };
    if !pattern_text.contains('*') {
        return Ok(vec![pattern]);
    }
    let parent = pattern
        .parent()
        .context("glob pattern must have a parent directory")?;
    let file_pattern = pattern
        .file_name()
        .and_then(|name| name.to_str())
        .context("glob pattern must end in a UTF-8 filename")?;
    let mut paths = Vec::new();
    for entry in fs::read_dir(parent).with_context(|| format!("read {}", parent.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if matches_glob_name(file_pattern, name) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn matches_glob_name(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    if let Some(first) = parts.first() {
        if !first.is_empty() {
            let Some(next) = rest.strip_prefix(first) else {
                return false;
            };
            rest = next;
        }
    }
    for part in parts.iter().skip(1).take(parts.len().saturating_sub(2)) {
        if part.is_empty() {
            continue;
        }
        let Some(idx) = rest.find(part) else {
            return false;
        };
        rest = &rest[idx + part.len()..];
    }
    if let Some(last) = parts.last() {
        last.is_empty() || rest.ends_with(last)
    } else {
        true
    }
}

fn clip_bytes(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    let cap = max_bytes.max(1);
    let clipped = bytes.len() > cap;
    let end = bytes.len().min(cap);
    let mut text = String::from_utf8_lossy(&bytes[..end]).to_string();
    while !text.is_char_boundary(text.len()) {
        text.pop();
    }
    (text, clipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_glob_inputs_in_order_and_clips() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "bbbbbb").unwrap();
        fs::write(dir.path().join("a.txt"), "aaaaaa").unwrap();
        fs::write(dir.path().join("ignore.md"), "mmmm").unwrap();
        let input = EvidenceInput {
            id: "plans".into(),
            kind: EvidenceInputKind::Glob,
            role: "target_plan".into(),
            path_or_url: "*.txt".into(),
            max_bytes: 3,
        };
        let loaded = load_evidence_inputs(dir.path(), &[input]).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded[0].source.ends_with("a.txt"));
        assert_eq!(loaded[0].content, "aaa");
        assert!(loaded[0].clipped);
    }

    #[test]
    fn url_evidence_is_disabled_by_default() {
        let input = EvidenceInput {
            id: "redline".into(),
            kind: EvidenceInputKind::Url,
            role: "parity_reference".into(),
            path_or_url: "https://example.com/summary.json".into(),
            max_bytes: 128,
        };
        let loaded = load_evidence_inputs(Path::new("."), &[input]).unwrap();
        assert_eq!(
            loaded[0].unavailable_reason.as_deref(),
            Some("url_evidence_disabled")
        );
        assert!(loaded[0].content.is_empty());
    }
}
