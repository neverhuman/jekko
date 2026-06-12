//! Shared split-family helpers for local checkout validation and common file
//! lookups.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use toml::Value as TomlValue;

use super::split_family::{SplitManifest, SplitRepo};

pub(crate) const EXPECTED_FAMILY: &str = "jekko-split";
pub(crate) const EXPECTED_AUDITOR_VERSION: &str = "1.6.1";

const EXPECTED_PORTAL_FILES: &[&str] = &[
    "AGENTS.md",
    "Cargo.toml",
    "Justfile",
    "README.md",
    ".github/workflows/jankurai.yml",
    "agent/JANKURAI_STANDARD.md",
    "agent/standard-version.toml",
    "crates/jekko-cli/src/main.rs",
    "crates/xtask/src/commands/jankurai_gate.rs",
    "crates/xtask/src/commands/split_family.rs",
    "jekko-split/repos.manifest.toml",
    "ops/ci/jankurai.sh",
    "scripts/split-sync.sh",
];
const TRANSIENT_STATUS_PREFIXES: &[&str] = &["?? apps/", "?? artifacts/", "?? node_modules/"];
const EXPECTED_SUPPORTING_FILES: &[&str] = &[
    "AGENTS.md",
    "Cargo.toml",
    "Justfile",
    "README.md",
    ".gitignore",
    ".gitlab-ci.yml",
    ".github/workflows/ci.yml",
    ".github/workflows/jankurai.yml",
    "agent/JANKURAI_STANDARD.md",
    "agent/generated-zones.toml",
    "agent/owner-map.json",
    "agent/standard-version.toml",
    "agent/test-map.json",
    "ops/ci/build.sh",
    "ops/ci/check.sh",
    "ops/ci/fast.sh",
    "ops/ci/jankurai.sh",
    "ops/ci/test.sh",
    "ops/ci/typecheck.sh",
    "src/lib.rs",
];

pub(crate) fn split_root_for(repo_root: &Path) -> Result<PathBuf> {
    if let Some(raw) = env::var_os("JEKKO_SPLIT_ROOT") {
        return Ok(PathBuf::from(raw));
    }

    if repo_root.file_name().and_then(|name| name.to_str()) == Some("jekko") {
        if let Some(parent) = repo_root.parent() {
            if parent.file_name().and_then(|name| name.to_str()) == Some(EXPECTED_FAMILY) {
                return Ok(parent.to_path_buf());
            }
        }
    }

    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(EXPECTED_FAMILY));
    }

    repo_root
        .parent()
        .map(|parent| parent.join(EXPECTED_FAMILY))
        .context("resolve split-family root")
}

pub(crate) fn validate_local_checkouts(split_root: &Path, manifest: &SplitManifest) -> Result<()> {
    for repo in &manifest.repo {
        let repo_path = split_root.join(&repo.path);
        if !repo_path.is_dir() {
            bail!("split repo missing locally: {}", repo_path.display());
        }
        ensure_git_eq(&repo_path, &["rev-parse", "--is-inside-work-tree"], "true")?;
        ensure_git_eq(&repo_path, &["branch", "--show-current"], "main")?;
        ensure_git_eq(
            &repo_path,
            &["remote", "get-url", "origin"],
            repo.remotes.origin.as_str(),
        )?;
        ensure_git_eq(
            &repo_path,
            &["remote", "get-url", "jeryu"],
            repo.remotes.jeryu.as_str(),
        )?;
        ensure_git_eq(
            &repo_path,
            &["remote", "get-url", "github"],
            repo.remotes.github.as_str(),
        )?;
        ensure_remote_main_ref(repo.remotes.jeryu.as_str())?;
        ensure_clean_checkout(&repo_path)?;
        for rel in expected_local_files(repo) {
            let path = repo_path.join(rel);
            if !path.exists() {
                bail!("split repo {} is missing required file {}", repo.path, rel);
            }
        }

        if gate_is_true(repo, "jankurai-1.6.1") {
            validate_jankurai_pin(&repo_path, repo)?;
        }
        if gate_is_true(repo, "audit-clean") {
            validate_audit_clean(&repo_path, repo)?;
        }
    }

    Ok(())
}

pub(crate) fn package_name(cargo_toml: &Path) -> Result<String> {
    let text =
        fs::read_to_string(cargo_toml).with_context(|| format!("read {}", cargo_toml.display()))?;
    let value: TomlValue =
        toml::from_str(&text).with_context(|| format!("parse {}", cargo_toml.display()))?;
    value
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no [package].name in {}", cargo_toml.display()))
}

pub(crate) fn ensure_eq<T>(label: &str, actual: T, expected: T) -> Result<()>
where
    T: PartialEq + std::fmt::Debug,
{
    if actual != expected {
        bail!("{label} mismatch: expected {expected:?}, found {actual:?}");
    }
    Ok(())
}

fn expected_local_files(repo: &SplitRepo) -> &'static [&'static str] {
    if repo.role == "portal" {
        EXPECTED_PORTAL_FILES
    } else {
        EXPECTED_SUPPORTING_FILES
    }
}

fn gate_is_true(repo: &SplitRepo, gate: &str) -> bool {
    repo.gates.get(gate).copied().unwrap_or(false)
}

fn validate_jankurai_pin(repo_path: &Path, repo: &SplitRepo) -> Result<()> {
    let standard_version = fs::read_to_string(repo_path.join("agent/standard-version.toml"))
        .with_context(|| {
            format!(
                "read agent/standard-version.toml in {}",
                repo_path.display()
            )
        })?;
    if !standard_version.contains(EXPECTED_AUDITOR_VERSION) {
        bail!(
            "split repo {} does not pin jankurai {} in agent/standard-version.toml",
            repo.path,
            EXPECTED_AUDITOR_VERSION
        );
    }
    if repo.role != "portal" {
        let agents = fs::read_to_string(repo_path.join("AGENTS.md"))
            .with_context(|| format!("read AGENTS.md in {}", repo_path.display()))?;
        if !agents.contains(EXPECTED_AUDITOR_VERSION) {
            bail!(
                "split repo {} AGENTS.md does not mention jankurai {}",
                repo.path,
                EXPECTED_AUDITOR_VERSION
            );
        }
    }
    Ok(())
}

fn validate_audit_clean(repo_path: &Path, repo: &SplitRepo) -> Result<()> {
    let score_path = repo_path.join("agent/repo-score.json");
    let text = fs::read_to_string(&score_path)
        .with_context(|| format!("read audit score {}", score_path.display()))?;
    let json: Value = serde_json::from_str(&text)
        .with_context(|| format!("parse audit score {}", score_path.display()))?;

    let auditor_version = json
        .get("auditor_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    if auditor_version != EXPECTED_AUDITOR_VERSION {
        bail!(
            "split repo {} audit score uses auditor_version {:?}, expected {:?}",
            repo.path,
            auditor_version,
            EXPECTED_AUDITOR_VERSION
        );
    }

    let blockers = json
        .get("conformance_blockers")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let hard_findings = hard_findings(&json);
    let caps = json
        .get("caps_applied")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if blockers > 0 || hard_findings > 0 || caps > 0 {
        bail!(
            "split repo {} audit is not clean: blockers={} hard_findings={} caps={}",
            repo.path,
            blockers,
            hard_findings,
            caps
        );
    }
    Ok(())
}

fn hard_findings(json: &Value) -> i64 {
    if let Some(top) = json.get("hard_findings").and_then(Value::as_i64) {
        return top;
    }
    if let Some(nested) = json
        .get("decision")
        .and_then(|decision| decision.get("hard_findings"))
        .and_then(Value::as_i64)
    {
        return nested;
    }
    json.get("findings")
        .and_then(Value::as_array)
        .map(|findings| {
            findings
                .iter()
                .filter(|finding| {
                    finding
                        .get("hardness")
                        .and_then(Value::as_str)
                        .is_some_and(|hardness| hardness == "hard")
                        || finding
                            .get("severity")
                            .and_then(Value::as_str)
                            .is_some_and(|severity| severity == "high" || severity == "critical")
                })
                .count() as i64
        })
        .unwrap_or(0)
}

fn ensure_remote_main_ref(remote_url: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["ls-remote", "--heads", remote_url, "main"])
        .output()
        .with_context(|| format!("run git ls-remote --heads {remote_url} main"))?;
    if !output.status.success() {
        bail!(
            "git ls-remote failed for {}: {}",
            remote_url,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        bail!("remote {} does not advertise refs/heads/main", remote_url);
    }
    Ok(())
}

fn ensure_git_eq(repo_path: &Path, args: &[&str], expected: &str) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .with_context(|| format!("run git {:?} in {}", args, repo_path.display()))?;
    if !output.status.success() {
        bail!(
            "git {:?} failed in {}: {}",
            args,
            repo_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let actual = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    ensure_eq(&format!("git {:?}", args), actual.as_str(), expected)?;
    Ok(())
}

fn ensure_clean_checkout(repo_path: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["status", "--short"])
        .output()
        .with_context(|| format!("run git status --short in {}", repo_path.display()))?;
    if !output.status.success() {
        bail!(
            "git status --short failed in {}: {}",
            repo_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let actual = String::from_utf8_lossy(&output.stdout);
    let unexpected: Vec<&str> = actual
        .lines()
        .map(str::trim_end)
        .filter(|line| {
            !line.is_empty()
                && !TRANSIENT_STATUS_PREFIXES
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
        })
        .collect();
    if !unexpected.is_empty() {
        bail!(
            "git [\"status\", \"--short\"] mismatch: expected clean checkout, found {}",
            unexpected.join("\\n")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_checkout_filter_keeps_real_dirty_entries() {
        let unexpected: Vec<&str> = "?? apps/\n?? artifacts/\n?? node_modules/\n M Cargo.toml\n"
            .lines()
            .map(str::trim_end)
            .filter(|line| {
                !line.is_empty()
                    && !TRANSIENT_STATUS_PREFIXES
                        .iter()
                        .any(|prefix| line.starts_with(prefix))
            })
            .collect();
        assert_eq!(unexpected, vec![" M Cargo.toml"]);
    }
}
