//! `xtask split-family-check` — validate the umbrella split-family registry.

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

const MANIFEST_PATH: &str = "jekko-split/repos.manifest.toml";
const EXPECTED_FAMILY: &str = "jekko-split";
const EXPECTED_UMBRELLA_REPO: &str = "jekko";
const EXPECTED_SCHEMA_VERSION: &str = "1.2.0";
const EXPECTED_IMPORT_BRANCH_SOURCE: &str = "import/source-20260610";
const EXPECTED_IMPORT_BRANCH_DIRTY: &str = "import/dirty-20260610";
const EXPECTED_AUDITOR_VERSION: &str = "1.6.1";
const EXPECTED_ONBOARDING_GATES: &[&str] = &[
    "remote-wired",
    "ci-skeleton",
    "jankurai-1.6.1",
    "audit-clean",
];
const EXPECTED_ROLLOUT_WAVE_ORDER: &[u8] = &[0, 1, 2, 3];
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
const EXPECTED_REPOS: &[SplitRepoSpec] = &[
    SplitRepoSpec {
        path: "jekko",
        role: "portal",
        wave: 0,
    },
    SplitRepoSpec {
        path: "jekko-core",
        role: "core",
        wave: 1,
    },
    SplitRepoSpec {
        path: "jekko-mcp",
        role: "mcp",
        wave: 1,
    },
    SplitRepoSpec {
        path: "jekko-deploy",
        role: "deploy",
        wave: 1,
    },
    SplitRepoSpec {
        path: "jekko-jnoccio",
        role: "router",
        wave: 2,
    },
    SplitRepoSpec {
        path: "jekko-jailgun",
        role: "web",
        wave: 2,
    },
    SplitRepoSpec {
        path: "jekko-zyal",
        role: "domain",
        wave: 2,
    },
    SplitRepoSpec {
        path: "jekko-search",
        role: "shared",
        wave: 3,
    },
    SplitRepoSpec {
        path: "jekko-memory",
        role: "data",
        wave: 3,
    },
    SplitRepoSpec {
        path: "jekko-agent",
        role: "agent",
        wave: 3,
    },
];

#[derive(Debug, Deserialize)]
struct SplitManifest {
    schema_version: String,
    family: String,
    umbrella_repo: String,
    import_branch_source: String,
    import_branch_dirty: String,
    onboarding_gates: Vec<String>,
    rollout_wave_order: Vec<u8>,
    repo: Vec<SplitRepo>,
}

#[derive(Debug, Deserialize)]
struct SplitRepo {
    path: String,
    name: String,
    slug: String,
    role: String,
    profile: String,
    branch: String,
    rollout_wave: u8,
    onboarded: bool,
    gates: BTreeMap<String, bool>,
    remotes: SplitRemotes,
}

#[derive(Debug, Deserialize)]
struct SplitRemotes {
    origin: String,
    jeryu: String,
    github: String,
}

#[derive(Debug, Clone, Copy)]
struct SplitRepoSpec {
    path: &'static str,
    role: &'static str,
    wave: u8,
}

pub fn run(repo_root: &Path) -> Result<()> {
    let manifest = load_manifest(repo_root)?;
    validate_manifest(&manifest)?;
    let split_root = split_root_for(repo_root)?;
    validate_local_checkouts(&split_root, &manifest)?;
    println!(
        "split-family-check: ✓ {} repos validated under {} across waves {:?}",
        manifest.repo.len(),
        split_root.display(),
        manifest.rollout_wave_order
    );
    Ok(())
}

fn load_manifest(repo_root: &Path) -> Result<SplitManifest> {
    let path = repo_root.join(MANIFEST_PATH);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parse TOML in {}", path.display()))
}

fn validate_manifest(manifest: &SplitManifest) -> Result<()> {
    ensure_eq(
        "schema_version",
        manifest.schema_version.as_str(),
        EXPECTED_SCHEMA_VERSION,
    )?;
    ensure_eq("family", manifest.family.as_str(), EXPECTED_FAMILY)?;
    ensure_eq(
        "umbrella_repo",
        manifest.umbrella_repo.as_str(),
        EXPECTED_UMBRELLA_REPO,
    )?;
    ensure_eq(
        "import_branch_source",
        manifest.import_branch_source.as_str(),
        EXPECTED_IMPORT_BRANCH_SOURCE,
    )?;
    ensure_eq(
        "import_branch_dirty",
        manifest.import_branch_dirty.as_str(),
        EXPECTED_IMPORT_BRANCH_DIRTY,
    )?;

    let onboarding_gates: Vec<&str> = manifest
        .onboarding_gates
        .iter()
        .map(String::as_str)
        .collect();
    ensure_eq(
        "onboarding_gates",
        onboarding_gates.as_slice(),
        EXPECTED_ONBOARDING_GATES,
    )?;
    ensure_eq(
        "rollout_wave_order",
        manifest.rollout_wave_order.as_slice(),
        EXPECTED_ROLLOUT_WAVE_ORDER,
    )?;

    if manifest.repo.len() != EXPECTED_REPOS.len() {
        bail!(
            "repo count mismatch: expected {} entries, found {}",
            EXPECTED_REPOS.len(),
            manifest.repo.len()
        );
    }

    let mut paths = HashSet::new();
    let mut names = HashSet::new();
    let mut slugs = HashSet::new();
    let mut roles = HashSet::new();

    for (index, repo) in manifest.repo.iter().enumerate() {
        let spec = EXPECTED_REPOS
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("unexpected repo at index {index}"))?;
        let expected_origin = expected_origin(spec.path);
        let expected_github = expected_github(spec.path);
        ensure_eq("path", repo.path.as_str(), spec.path)?;
        ensure_eq("name", repo.name.as_str(), spec.path)?;
        ensure_eq("slug", repo.slug.as_str(), spec.path)?;
        ensure_eq("role", repo.role.as_str(), spec.role)?;
        ensure_eq("branch", repo.branch.as_str(), "main")?;
        ensure_eq(
            "profile",
            repo.profile.as_str(),
            expected_profile(spec.path),
        )?;
        ensure_eq("rollout_wave", repo.rollout_wave, spec.wave)?;
        validate_repo_gates(repo)?;

        ensure_eq(
            "origin remote",
            repo.remotes.origin.as_str(),
            expected_origin.as_str(),
        )?;
        ensure_eq(
            "jeryu remote",
            repo.remotes.jeryu.as_str(),
            expected_origin.as_str(),
        )?;
        ensure_eq(
            "github remote",
            repo.remotes.github.as_str(),
            expected_github.as_str(),
        )?;

        if !paths.insert(repo.path.clone()) {
            bail!("duplicate path detected: {}", repo.path);
        }
        if !names.insert(repo.name.clone()) {
            bail!("duplicate name detected: {}", repo.name);
        }
        if !slugs.insert(repo.slug.clone()) {
            bail!("duplicate slug detected: {}", repo.slug);
        }
        if !roles.insert(repo.role.clone()) {
            bail!("duplicate role detected: {}", repo.role);
        }
    }

    Ok(())
}

fn validate_repo_gates(repo: &SplitRepo) -> Result<()> {
    let mut keys: Vec<&str> = repo.gates.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut expected = EXPECTED_ONBOARDING_GATES.to_vec();
    expected.sort_unstable();
    ensure_eq("repo gates", keys.as_slice(), expected.as_slice())?;

    let all_gates_passed = EXPECTED_ONBOARDING_GATES
        .iter()
        .all(|gate| repo.gates.get(*gate).copied().unwrap_or(false));
    if repo.onboarded != all_gates_passed {
        bail!(
            "repo {} onboarded={} but gates imply onboarded={}",
            repo.path,
            repo.onboarded,
            all_gates_passed
        );
    }
    if !repo.onboarded {
        bail!("repo {} is not fully onboarded", repo.path);
    }
    Ok(())
}

fn split_root_for(repo_root: &Path) -> Result<PathBuf> {
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

fn validate_local_checkouts(split_root: &Path, manifest: &SplitManifest) -> Result<()> {
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
        ensure_git_eq(&repo_path, &["status", "--short"], "")?;
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
    let agents = fs::read_to_string(repo_path.join("AGENTS.md"))
        .with_context(|| format!("read AGENTS.md in {}", repo_path.display()))?;
    if !agents.contains(EXPECTED_AUDITOR_VERSION) {
        bail!(
            "split repo {} AGENTS.md does not mention jankurai {}",
            repo.path,
            EXPECTED_AUDITOR_VERSION
        );
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

fn ensure_eq<T>(label: &str, actual: T, expected: T) -> Result<()>
where
    T: PartialEq + std::fmt::Debug,
{
    if actual != expected {
        bail!("{label} mismatch: expected {expected:?}, found {actual:?}");
    }
    Ok(())
}

fn expected_origin(repo: &str) -> String {
    format!("http://127.0.0.1:8787/git/jeryu/{repo}.git")
}

fn expected_github(repo: &str) -> String {
    format!("https://github.com/neverhuman/{repo}.git")
}

fn expected_profile(repo: &str) -> &'static str {
    match repo {
        "jekko" => "rust-portal",
        "jekko-core" => "rust-core",
        "jekko-mcp" => "rust-mcp",
        "jekko-deploy" => "ops",
        "jekko-jnoccio" => "rust-router",
        "jekko-jailgun" => "rust-web",
        "jekko-zyal" => "rust-domain",
        "jekko-search" => "rust-shared",
        "jekko-memory" => "rust-data",
        "jekko-agent" => "rust-agent",
        other => panic!("unexpected split repo: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_the_canonical_manifest() {
        let text = include_str!("../../../../jekko-split/repos.manifest.toml");
        let manifest: SplitManifest = toml::from_str(text).expect("parse canonical manifest");
        validate_manifest(&manifest).expect("manifest validates");
    }

    #[test]
    fn rejects_bad_remote_urls() {
        let text = include_str!("../../../../jekko-split/repos.manifest.toml");
        let mutated = text.replacen(
            "https://github.com/neverhuman/jekko-mcp.git",
            "https://github.com/neverhuman/jekko-mcp-wrong.git",
            1,
        );
        let manifest: SplitManifest = toml::from_str(&mutated).expect("parse mutated manifest");
        let err = validate_manifest(&manifest).expect_err("bad remote should fail");
        assert!(err.to_string().contains("github remote"));
    }
}
