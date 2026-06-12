//! `xtask split-ownership-check` — validate the crate→repo ownership map
//! (`jekko-split/crate-ownership.toml`) and, optionally, that each split-family
//! child repo actually contains the crates it is supposed to own.
//!
//! This closes the blind spot in `split-family-check`, which validates scaffold
//! files, wired remotes, and onboarding gates but never verifies that any
//! product crate landed — so empty child repos report `onboarded = true`.
//!
//! The map itself is always validated:
//! - schema/family/umbrella header constants,
//! - no crate owned by two repos,
//! - the universe invariant (owned non-planned crates == umbrella workspace
//!   members plus the real non-member crates, nothing extra or missing),
//! - every owned source crate exists in the umbrella with a matching package
//!   name,
//! - `exports` are a subset of the owning repo's crates, and
//! - `git_deps` are known repos at an equal-or-earlier wave (the portal
//!   aggregates everything and is exempt because it lands last).
//!
//! With `--require-landed` the command additionally fails when any owned,
//! non-planned crate is missing from its child checkout — the real
//! "is the split finished?" gate used at final cutover.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use super::split_common::{ensure_eq, package_name, split_root_for};

const OWNERSHIP_PATH: &str = "jekko-split/crate-ownership.toml";
const EXPECTED_SCHEMA_VERSION: &str = "1.0.0";
const EXPECTED_FAMILY: &str = "jekko-split";
const EXPECTED_UMBRELLA_REPO: &str = "jekko";

/// Real crates that live under `crates/` but are intentionally not members of
/// the umbrella workspace; they still belong to exactly one child repo.
const EXTRA_REAL_CRATES: &[&str] = &["crates/agent-search", "crates/cogcore"];

#[derive(Debug, Deserialize)]
struct Ownership {
    schema_version: String,
    family: String,
    umbrella_repo: String,
    repo: Vec<OwnRepo>,
}

#[derive(Debug, Deserialize)]
struct OwnRepo {
    name: String,
    role: String,
    wave: u8,
    owns: Vec<OwnedCrate>,
    #[serde(default)]
    exports: Vec<String>,
    #[serde(default)]
    git_deps: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OwnedCrate {
    #[serde(rename = "crate")]
    name: String,
    src: String,
    dest: String,
    #[serde(default)]
    planned: bool,
}

struct RepoLanding {
    name: String,
    checkable: Vec<String>,
    missing: Vec<String>,
}

pub fn run(repo_root: &Path, require_landed: bool) -> Result<()> {
    let ownership = load_ownership(repo_root)?;
    validate_map(&ownership, repo_root)?;

    let split_root = split_root_for(repo_root)?;
    let report = landing_report(&ownership, &split_root)?;

    let mut landed_repos = 0usize;
    let mut total_missing = 0usize;
    for repo in &report {
        total_missing += repo.missing.len();
        let status = if repo.checkable.is_empty() {
            "no landable crates".to_string()
        } else if repo.missing.is_empty() {
            landed_repos += 1;
            format!("landed ({} crate(s))", repo.checkable.len())
        } else {
            format!(
                "SCAFFOLD ({}/{} missing: {})",
                repo.missing.len(),
                repo.checkable.len(),
                repo.missing.join(", ")
            )
        };
        println!("  {:<14} {status}", repo.name);
    }

    let owned_total: usize = report.iter().map(|r| r.checkable.len()).sum();
    println!(
        "split-ownership-check: ✓ map valid — {} repos, {} crate(s) owned; {}/{} repos fully landed",
        ownership.repo.len(),
        owned_total,
        landed_repos,
        report.len(),
    );

    if require_landed && total_missing > 0 {
        bail!(
            "split not complete: {total_missing} owned crate(s) not yet landed in their child repos — run the wave migrations"
        );
    }
    Ok(())
}

fn load_ownership(repo_root: &Path) -> Result<Ownership> {
    let path = repo_root.join(OWNERSHIP_PATH);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parse TOML in {}", path.display()))
}

fn validate_map(ownership: &Ownership, repo_root: &Path) -> Result<()> {
    ensure_eq(
        "schema_version",
        ownership.schema_version.as_str(),
        EXPECTED_SCHEMA_VERSION,
    )?;
    ensure_eq("family", ownership.family.as_str(), EXPECTED_FAMILY)?;
    ensure_eq(
        "umbrella_repo",
        ownership.umbrella_repo.as_str(),
        EXPECTED_UMBRELLA_REPO,
    )?;

    // 1. unique repos/roles; no crate owned by two repos; exports ⊆ owned.
    let mut owned_once = BTreeSet::new();
    let mut repo_names = BTreeSet::new();
    let mut roles = BTreeSet::new();
    let mut wave_of: BTreeMap<String, u8> = BTreeMap::new();
    for repo in &ownership.repo {
        if !repo_names.insert(repo.name.clone()) {
            bail!("duplicate repo in ownership map: {}", repo.name);
        }
        if !roles.insert(repo.role.clone()) {
            bail!("duplicate role in ownership map: {}", repo.role);
        }
        wave_of.insert(repo.name.clone(), repo.wave);

        let owned_here: BTreeSet<&str> = repo.owns.iter().map(|c| c.name.as_str()).collect();
        for c in &repo.owns {
            if !owned_once.insert(c.name.clone()) {
                bail!("crate owned by more than one repo: {}", c.name);
            }
        }
        for export in &repo.exports {
            if !owned_here.contains(export.as_str()) {
                bail!(
                    "repo {} exports {} which it does not own",
                    repo.name,
                    export
                );
            }
        }
    }

    // 2. git_deps reference known repos, are not self, and are wave-legal.
    //    The portal aggregates every child and lands last, so it is exempt.
    for repo in &ownership.repo {
        for dep in &repo.git_deps {
            if dep == &repo.name {
                bail!("repo {} lists itself in git_deps", repo.name);
            }
            let dep_wave = wave_of
                .get(dep)
                .ok_or_else(|| anyhow!("repo {} git_deps unknown repo {}", repo.name, dep))?;
            if repo.role != "portal" && *dep_wave > repo.wave {
                bail!(
                    "repo {} (wave {}) git_deps {} (wave {}): may only depend on equal-or-earlier waves",
                    repo.name,
                    repo.wave,
                    dep,
                    dep_wave
                );
            }
        }
    }

    // 3. universe invariant: owned non-planned crates == umbrella package set.
    let umbrella = umbrella_packages(repo_root)?;
    let owned_real: BTreeSet<String> = ownership
        .repo
        .iter()
        .flat_map(|r| r.owns.iter())
        .filter(|c| !c.planned)
        .map(|c| c.name.clone())
        .collect();
    let umbrella_but_unowned: Vec<&String> = umbrella.difference(&owned_real).collect();
    let owned_but_not_umbrella: Vec<&String> = owned_real.difference(&umbrella).collect();
    if !umbrella_but_unowned.is_empty() || !owned_but_not_umbrella.is_empty() {
        bail!(
            "ownership universe mismatch — umbrella-but-unowned: {umbrella_but_unowned:?}; owned-but-not-in-umbrella: {owned_but_not_umbrella:?}"
        );
    }

    // 4. each non-planned owned crate exists at `src` with a matching package name.
    for repo in &ownership.repo {
        for c in &repo.owns {
            if c.planned {
                continue;
            }
            let cargo = repo_root.join(&c.src).join("Cargo.toml");
            let name = package_name(&cargo)
                .with_context(|| format!("repo {} crate {}", repo.name, c.name))?;
            if name != c.name {
                bail!(
                    "ownership src mismatch: {} declares crate {} but {} has package name {}",
                    repo.name,
                    c.name,
                    cargo.display(),
                    name
                );
            }
        }
    }
    Ok(())
}

fn landing_report(ownership: &Ownership, split_root: &Path) -> Result<Vec<RepoLanding>> {
    let mut out = Vec::with_capacity(ownership.repo.len());
    for repo in &ownership.repo {
        let mut checkable = Vec::new();
        let mut missing = Vec::new();
        for c in &repo.owns {
            if c.planned {
                continue;
            }
            checkable.push(c.name.clone());
            let cargo = split_root.join(&repo.name).join(&c.dest).join("Cargo.toml");
            let landed = package_name(&cargo).map(|n| n == c.name).unwrap_or(false);
            if !landed {
                missing.push(c.name.clone());
            }
        }
        out.push(RepoLanding {
            name: repo.name.clone(),
            checkable,
            missing,
        });
    }
    Ok(out)
}

fn umbrella_packages(repo_root: &Path) -> Result<BTreeSet<String>> {
    let cargo_path = repo_root.join("Cargo.toml");
    let text = fs::read_to_string(&cargo_path)
        .with_context(|| format!("read {}", cargo_path.display()))?;
    let value: toml::Value =
        toml::from_str(&text).with_context(|| format!("parse {}", cargo_path.display()))?;
    let members = value
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .ok_or_else(|| anyhow!("no [workspace].members in {}", cargo_path.display()))?;

    let mut dirs: Vec<String> = members
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    for extra in EXTRA_REAL_CRATES {
        dirs.push((*extra).to_string());
    }

    let mut pkgs = BTreeSet::new();
    for dir in dirs {
        let cargo = repo_root.join(&dir).join("Cargo.toml");
        let name = package_name(&cargo).with_context(|| format!("workspace member {dir}"))?;
        pkgs.insert(name);
    }
    Ok(pkgs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn umbrella_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("canonicalize umbrella root")
    }

    #[test]
    fn canonical_ownership_map_validates() {
        let root = umbrella_root();
        let ownership = load_ownership(&root).expect("load ownership");
        validate_map(&ownership, &root).expect("canonical ownership map validates");
    }

    #[test]
    fn rejects_double_ownership() {
        let root = umbrella_root();
        let mut ownership = load_ownership(&root).unwrap();
        let dup = ownership.repo[1].owns[0].name.clone();
        ownership.repo[0].owns.push(OwnedCrate {
            name: dup,
            src: "crates/jekko-core".into(),
            dest: "crates/jekko-core".into(),
            planned: false,
        });
        let err = validate_map(&ownership, &root).expect_err("double ownership must be rejected");
        assert!(err.to_string().contains("more than one repo"), "{err}");
    }

    #[test]
    fn rejects_crate_outside_umbrella() {
        let root = umbrella_root();
        let mut ownership = load_ownership(&root).unwrap();
        ownership.repo[0].owns.push(OwnedCrate {
            name: "phantom-crate".into(),
            src: "crates/phantom-crate".into(),
            dest: "crates/phantom-crate".into(),
            planned: false,
        });
        let err = validate_map(&ownership, &root).expect_err("unknown crate must be rejected");
        assert!(err.to_string().contains("universe"), "{err}");
    }

    #[test]
    fn rejects_wave_inversion() {
        let root = umbrella_root();
        let mut ownership = load_ownership(&root).unwrap();
        // Make a wave-1 repo depend on a wave-3 repo (illegal; only portal may).
        let core = ownership
            .repo
            .iter_mut()
            .find(|r| r.name == "jekko-core")
            .unwrap();
        core.git_deps.push("jekko-agent".into());
        let err = validate_map(&ownership, &root).expect_err("wave inversion must be rejected");
        assert!(err.to_string().contains("equal-or-earlier waves"), "{err}");
    }
}
