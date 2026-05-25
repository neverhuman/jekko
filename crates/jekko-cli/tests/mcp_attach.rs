//! `jekko mcp attach` integration tests.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn jekko() -> Command {
    Command::cargo_bin("jekko").expect("jekko binary")
}

#[test]
fn attach_writes_servers_entry() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "python"])
        .arg("--")
        .args(["-m", "apps.mcp_server", "--transport", "stdio"])
        .assert()
        .success()
        .stdout(predicate::str::contains("attached mcp server `aara`"));

    let body = fs::read_to_string(&cfg).unwrap();
    assert!(body.contains("[servers.aara]"));
    assert!(body.contains(r#"transport = "stdio""#));
    assert!(body.contains(r#"command = "python""#));
    assert!(body.contains(r#""apps.mcp_server""#));
}

#[test]
fn attach_refuses_duplicate_name() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "echo"])
        .assert()
        .success();
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "cat"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn attach_force_replaces() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "echo"])
        .assert()
        .success();
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "--force", "cat"])
        .assert()
        .success();
    let body = fs::read_to_string(&cfg).unwrap();
    assert!(body.contains(r#"command = "cat""#));
    assert!(!body.contains(r#"command = "echo""#));
}

#[test]
fn attach_rejects_bad_name() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    for bad in ["aara.prod", "aara/prod", "aara prod", "aara;rm -rf /"] {
        jekko()
            .args(["mcp", "attach", "--config"])
            .arg(&cfg)
            .args([bad, "echo"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("rejected"));
    }
}

#[test]
fn attach_rejects_unknown_transport() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "--transport", "websocket", "ws://x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown transport"));
}

#[test]
fn attach_sse_accepts_url_target() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["remote", "--transport", "sse", "http://aara.local/mcp/sse"])
        .assert()
        .success();
    let body = fs::read_to_string(&cfg).unwrap();
    assert!(body.contains(r#"transport = "sse""#));
    assert!(body.contains(r#"command = "http://aara.local/mcp/sse""#));
}

#[test]
fn attach_creates_parent_dirs() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("nested/deeper/mcp.toml");
    assert!(!cfg.parent().unwrap().exists());
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "echo"])
        .assert()
        .success();
    assert!(cfg.exists());
}
