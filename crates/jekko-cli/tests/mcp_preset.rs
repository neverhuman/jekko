//! `jekko mcp preset` integration tests.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn jekko() -> Command {
    Command::cargo_bin("jekko").expect("jekko binary")
}

#[test]
fn preset_list_shows_eight_canonical_presets() {
    let out = jekko()
        .args(["mcp", "preset", "list"])
        .output()
        .expect("preset list");
    let stdout = String::from_utf8(out.stdout).unwrap();
    for name in [
        "aws",
        "gdrive",
        "github",
        "huggingface",
        "kubernetes",
        "linear",
        "openai",
        "vercel",
    ] {
        assert!(
            stdout.contains(name),
            "preset `{name}` missing from list output:\n{stdout}"
        );
    }
    // `claude` is intentionally absent — no canonical Claude MCP server.
    assert!(
        !stdout.lines().any(|l| l.starts_with("claude ")),
        "claude must not appear as a preset row:\n{stdout}"
    );
    assert!(stdout.contains("NAME"));
    assert!(stdout.contains("DESCRIPTION"));
    assert!(stdout.contains("ENV"));
}

#[test]
fn preset_add_github_writes_correct_stanza() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "preset", "add", "--config"])
        .arg(&cfg)
        .args(["github"])
        .assert()
        .success()
        .stdout(predicate::str::contains("attached preset `github`"));

    let body = fs::read_to_string(&cfg).unwrap();
    assert!(body.contains("[servers.github]"));
    assert!(body.contains(r#"transport = "stdio""#));
    assert!(body.contains(r#"command = "npx""#));
    assert!(body.contains(r#""@modelcontextprotocol/server-github""#));
    // Critically: the env var is a placeholder, NOT a resolved secret.
    assert!(body.contains(r#"GITHUB_PERSONAL_ACCESS_TOKEN = "${GITHUB_PERSONAL_ACCESS_TOKEN}""#));
}

#[test]
fn preset_add_with_rename_uses_custom_server_name() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "preset", "add", "--config"])
        .arg(&cfg)
        .args(["linear", "--as", "linear_prod"])
        .assert()
        .success();

    let body = fs::read_to_string(&cfg).unwrap();
    assert!(body.contains("[servers.linear_prod]"));
    assert!(!body.contains("[servers.linear]"));
}

#[test]
fn preset_add_unknown_name_errors() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "preset", "add", "--config"])
        .arg(&cfg)
        .args(["ghost-preset"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown preset `ghost-preset`"));
}

#[test]
fn preset_add_duplicate_refuses_without_force() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "preset", "add", "--config"])
        .arg(&cfg)
        .args(["aws"])
        .assert()
        .success();
    jekko()
        .args(["mcp", "preset", "add", "--config"])
        .arg(&cfg)
        .args(["aws"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
    // --force allows replace.
    jekko()
        .args(["mcp", "preset", "add", "--config"])
        .arg(&cfg)
        .args(["aws", "--force"])
        .assert()
        .success();
}

#[test]
fn preset_add_warns_when_required_env_missing() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    let out = jekko()
        .env_remove("LINEAR_API_KEY")
        .args(["mcp", "preset", "add", "--config"])
        .arg(&cfg)
        .args(["linear"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "preset add must succeed despite missing env"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("LINEAR_API_KEY"),
        "missing-env warning must name the variable:\n{stderr}"
    );
}

#[test]
fn preset_add_followed_by_list_round_trip() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    for preset in ["aws", "github", "linear"] {
        jekko()
            .args(["mcp", "preset", "add", "--config"])
            .arg(&cfg)
            .args([preset])
            .assert()
            .success();
    }
    let list = jekko()
        .args(["mcp", "list", "--config"])
        .arg(&cfg)
        .output()
        .unwrap();
    let stdout = String::from_utf8(list.stdout).unwrap();
    assert!(stdout.contains("aws"));
    assert!(stdout.contains("github"));
    assert!(stdout.contains("linear"));
    assert!(stdout.contains("npx"));
    assert!(stdout.contains("uvx"));
}
