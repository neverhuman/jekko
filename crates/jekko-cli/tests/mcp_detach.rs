//! `jekko mcp detach` integration tests.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn jekko() -> Command {
    Command::cargo_bin("jekko").expect("jekko binary")
}

#[test]
fn detach_removes_servers_entry() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "echo"])
        .assert()
        .success();

    jekko()
        .args(["mcp", "detach", "--config"])
        .arg(&cfg)
        .args(["aara"])
        .assert()
        .success()
        .stdout(predicate::str::contains("detached mcp server `aara`"));

    let body = fs::read_to_string(&cfg).unwrap_or_default();
    assert!(!body.contains("[servers.aara]"));
}

#[test]
fn detach_unknown_name_errors() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    fs::write(
        &cfg,
        r#"
[servers.alpha]
transport = "stdio"
command = "echo"
args = []
"#,
    )
    .unwrap();
    jekko()
        .args(["mcp", "detach", "--config"])
        .arg(&cfg)
        .args(["ghost"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found in config"));
}

#[test]
fn detach_preserves_other_entries_and_comments() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    // Order matters: a leading comment attaches to the section that
    // immediately follows it in `toml_edit`'s model, even across blank
    // lines. Put `beta` first (the entry that will REMAIN) so the comment
    // is anchored to it and survives `detach alpha`.
    fs::write(
        &cfg,
        r#"# preserved-top
[servers.beta]
transport = "stdio"
command = "cat"
args = []

[servers.alpha]
transport = "stdio"
command = "echo"
args = []
"#,
    )
    .unwrap();
    jekko()
        .args(["mcp", "detach", "--config"])
        .arg(&cfg)
        .args(["alpha"])
        .assert()
        .success();
    let body = fs::read_to_string(&cfg).unwrap();
    assert!(body.contains("# preserved-top"));
    assert!(!body.contains("[servers.alpha]"));
    assert!(body.contains("[servers.beta]"));
}

#[test]
fn detach_rejects_bad_name() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "detach", "--config"])
        .arg(&cfg)
        .args(["../etc"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("rejected"));
}
