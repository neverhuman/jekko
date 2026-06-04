use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;

use crate::shared::repo_root;

#[derive(Copy, Clone, Debug, Eq, PartialEq, clap::ValueEnum)]
pub(crate) enum GuardMode {
    Advisory,
    Final,
}

#[derive(Debug, Serialize)]
struct GuardHit {
    path: String,
    pattern: String,
}

pub(crate) fn run_preflight() -> Result<()> {
    let root = repo_root()?;
    println!("preflight: Rust TUI readiness report\n");

    let mut failures: Vec<String> = Vec::new();

    let gh_auth = ProcessCommand::new("gh")
        .args(["auth", "token"])
        .output()
        .context("run gh auth token")?;
    let gh_auth_ok = gh_auth.status.success() && !gh_auth.stdout.is_empty();
    println!(
        "  [{}] authenticated gh token is available",
        if gh_auth_ok { "OK" } else { "FAIL" }
    );
    if !gh_auth_ok {
        failures.push("authenticated gh token is required for PR-policy parity".into());
    }

    let metadata_ok = ProcessCommand::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(&root)
        .output()
        .context("run cargo metadata")?
        .status
        .success();
    println!(
        "  [{}] cargo metadata resolves",
        if metadata_ok { "OK" } else { "FAIL" }
    );
    if !metadata_ok {
        failures.push("cargo metadata failed".into());
    }

    let manifest = fs::read_to_string(root.join("Cargo.toml")).context("read Cargo.toml")?;
    let vendored_audit_crate = ["\"crates/jan", "kurai\""].concat();
    let vendored_runner_crate = ["\"crates/jan", "kurai-runner\""].concat();
    let vendored_jankurai =
        manifest.contains(&vendored_audit_crate) || manifest.contains(&vendored_runner_crate);
    println!(
        "  [OK] Jankurai workspace members are {}",
        if vendored_jankurai {
            "native source crates"
        } else {
            "not detected"
        }
    );

    let jankurai_version = ProcessCommand::new("jankurai")
        .arg("--version")
        .output()
        .context("run jankurai --version")?;
    let jankurai_ok = jankurai_version.status.success()
        && String::from_utf8_lossy(&jankurai_version.stdout).contains("1.5.1");
    println!(
        "  [{}] installed jankurai is v1.5.1",
        if jankurai_ok { "OK" } else { "FAIL" }
    );
    if !jankurai_ok {
        failures.push("installed jankurai is not v1.5.1".into());
    }

    println!();
    if failures.is_empty() {
        println!("preflight: PASS");
        Ok(())
    } else {
        println!("preflight: FAIL — {} blocker(s) remain:", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        bail!("preflight: {} unresolved blocker(s)", failures.len());
    }
}

pub(crate) fn guard_forbidden_runtime(mode: GuardMode) -> Result<()> {
    let root = repo_root()?;
    let forbidden_lock = ["b", "un.lock"].concat();
    let forbidden_test = ["b", "un:test"].concat();
    let forbidden_namespace = ["b", "un:"].concat();
    let forbidden_config = ["b", "unfig"].concat();
    let forbidden_types = ["@types/", "b", "un"].concat();
    let forbidden_tsconfig = ["@tsconfig/", "b", "un"].concat();
    let forbidden_bundle_pkg = ["@vi", "te"].concat();
    let forbidden_bundle_config = ["vi", "te.config"].concat();
    let exact_patterns = [
        "@opentui",
        "opentui-spinner",
        "solid-js",
        forbidden_test.as_str(),
        forbidden_namespace.as_str(),
        forbidden_config.as_str(),
        forbidden_lock.as_str(),
        forbidden_types.as_str(),
        forbidden_tsconfig.as_str(),
        forbidden_bundle_pkg.as_str(),
        forbidden_bundle_config.as_str(),
    ];
    let token_bun = ["b", "un"].concat();
    let token_bun_upper = ["B", "un"].concat();
    let token_vite = ["vi", "te"].concat();
    let token_patterns = [
        token_bun.as_str(),
        token_bun_upper.as_str(),
        token_vite.as_str(),
    ];
    let allow_prefixes = [
        root.join("target"),
        root.join(".git"),
        root.join(".vscode"),
        root.join("tips"),
        root.join("paper"),
        root.join("smartmemory"),
        root.join("specs"),
        root.join("db/migrations"),
        root.join("docs/archive"),
        root.join("docs/ZYAL"),
        root.join("docs/ci-local.md"),
        root.join("docs/ZYAL_MISSION.md"),
        root.join("achat.md"),
        root.join("agent_chat.md"),
        root.join("SANDBOX_WORKPLAN.md"),
        root.join("STATS.md"),
        root.join("ZYAL_MISSION.md"),
        root.join("CHANGELOG.md"),
        root.join("README.md"),
        root.join("CONTRIBUTING.md"),
        root.join("UNLOCK_WORKPLAN.md"),
        root.join("ZYAL_WORKFLOW.md"),
        root.join("JANKURAI_TASKLIST.md"),
        root.join("agent/TUI_UPGRADE.md"),
        root.join("agent/proofs"),
        root.join("agent/baselines"),
        root.join(".jankurai"),
        root.join("agent/owner-map.json"),
        root.join("agent/standard-version.toml"),
        root.join("agent/audit-policy.toml"),
        root.join("agent/jankurai-install.toml"),
        root.join("agent/generated-zones.toml"),
        root.join("crates/memory-benchmark/data"),
        root.join("crates/memory-benchmark/README.md"),
        root.join("crates/xtask"),
        root.join("jnoccio-fusion/ENCRYPTION.md"),
        root.join("jnoccio-fusion/KEYS.md"),
    ];

    let mut hits = Vec::new();
    let output = ProcessCommand::new("git")
        .args(["ls-files", "-co", "--exclude-standard", "--full-name"])
        .current_dir(&root)
        .output()
        .context("running git ls-files")?;

    if !output.status.success() {
        bail!("git ls-files failed with status {}", output.status);
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let path = root.join(line);
        if allow_prefixes.iter().any(|prefix| path.starts_with(prefix)) {
            continue;
        }
        if !is_text_candidate(&path) {
            continue;
        }
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        for pattern in exact_patterns {
            if text.contains(pattern) {
                hits.push(GuardHit {
                    path: path
                        .strip_prefix(&root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                    pattern: pattern.to_string(),
                });
            }
        }
        for pattern in token_patterns {
            if contains_ascii_token(&text, pattern) {
                hits.push(GuardHit {
                    path: path
                        .strip_prefix(&root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                    pattern: pattern.to_string(),
                });
            }
        }
    }

    if hits.is_empty() {
        println!("no forbidden runtime references found");
        return Ok(());
    }

    for hit in &hits {
        println!("{}: {}", hit.path, hit.pattern);
    }

    if mode == GuardMode::Final {
        bail!("forbidden runtime references found: {}", hits.len());
    }

    Ok(())
}

fn contains_ascii_token(text: &str, needle: &str) -> bool {
    let mut offset = 0;
    while let Some(relative) = text[offset..].find(needle) {
        let start = offset + relative;
        let end = start + needle.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if is_token_boundary(before) && is_token_boundary(after) {
            return true;
        }
        offset = end;
    }
    false
}

fn is_token_boundary(ch: Option<char>) -> bool {
    match ch {
        None => true,
        Some(ch) => !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'),
    }
}

fn is_text_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|v| v.to_str()),
        Some(
            "rs" | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "json"
                | "jsonc"
                | "toml"
                | "md"
                | "yml"
                | "yaml"
                | "sh"
                | "nix"
        )
    )
}
