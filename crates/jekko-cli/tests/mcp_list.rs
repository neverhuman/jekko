//! `jekko mcp list` integration tests.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn jekko() -> Command {
    Command::cargo_bin("jekko").expect("jekko binary")
}

#[test]
fn list_empty_config_succeeds_with_message() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "list", "--config"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(predicate::str::contains("no mcp servers configured"));
}

#[test]
fn list_after_attach_shows_entry() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "python"])
        .arg("--")
        .args(["-m", "apps.mcp_server"])
        .assert()
        .success();

    jekko()
        .args(["mcp", "list", "--config"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(predicate::str::contains("aara"))
        .stdout(predicate::str::contains("stdio"))
        .stdout(predicate::str::contains("python"))
        .stdout(predicate::str::contains("apps.mcp_server"));
}

#[test]
fn list_no_config_path_with_no_env_errors_helpfully() {
    let tmp = TempDir::new().unwrap();
    // Sanity: ensure file does not exist.
    let cfg = tmp.path().join("mcp.toml");
    assert!(!cfg.exists());

    // With JEKKO_HOME pointing at our tmp dir, list should succeed
    // (default path resolves, file is missing, list reports empty).
    jekko()
        .env("JEKKO_HOME", tmp.path())
        .env_remove("HOME")
        .args(["mcp", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no mcp servers configured"));
}

#[test]
fn list_header_row_present_when_entries_exist() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    // Seed config directly to avoid relying on attach.
    fs::write(
        &cfg,
        r#"
[servers.alpha]
transport = "stdio"
command = "echo"
args = ["hi"]

[servers.beta]
transport = "stdio"
command = "cat"
args = []
"#,
    )
    .unwrap();

    let output = jekko()
        .args(["mcp", "list", "--config"])
        .arg(&cfg)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("NAME"));
    assert!(stdout.contains("TRANSPORT"));
    assert!(stdout.contains("COMMAND"));
    assert!(stdout.contains("alpha"));
    assert!(stdout.contains("beta"));
    // Alphabetical ordering (BTreeMap).
    assert!(stdout.find("alpha").unwrap() < stdout.find("beta").unwrap());
}
