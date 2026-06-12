//! `xtask split-family-check` — validate the umbrella split-family registry.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::split_common::{ensure_eq, split_root_for, validate_local_checkouts, EXPECTED_FAMILY};

const MANIFEST_PATH: &str = "jekko-split/repos.manifest.toml";
const EXPECTED_UMBRELLA_REPO: &str = "jekko";
const EXPECTED_SCHEMA_VERSION: &str = "1.2.0";
const EXPECTED_IMPORT_BRANCH_SOURCE: &str = "import/source-20260610";
const EXPECTED_IMPORT_BRANCH_DIRTY: &str = "import/dirty-20260610";
const EXPECTED_ONBOARDING_GATES: &[&str] = &[
    "remote-wired",
    "ci-skeleton",
    "jankurai-1.6.1",
    "audit-clean",
];
const EXPECTED_ROLLOUT_WAVE_ORDER: &[u8] = &[0, 1, 2, 3];
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
pub(crate) struct SplitManifest {
    pub(crate) schema_version: String,
    pub(crate) family: String,
    pub(crate) umbrella_repo: String,
    pub(crate) import_branch_source: String,
    pub(crate) import_branch_dirty: String,
    pub(crate) onboarding_gates: Vec<String>,
    pub(crate) rollout_wave_order: Vec<u8>,
    pub(crate) repo: Vec<SplitRepo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SplitRepo {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) slug: String,
    pub(crate) role: String,
    pub(crate) profile: String,
    pub(crate) branch: String,
    pub(crate) rollout_wave: u8,
    pub(crate) onboarded: bool,
    pub(crate) gates: BTreeMap<String, bool>,
    pub(crate) remotes: SplitRemotes,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SplitRemotes {
    pub(crate) origin: String,
    pub(crate) jeryu: String,
    pub(crate) github: String,
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

// Shared checkout/root helpers live in `split_common` so this file stays
// focused on the manifest shape and wave validation.

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
