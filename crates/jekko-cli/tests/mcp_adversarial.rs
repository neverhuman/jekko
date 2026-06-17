//! Adversarial fixtures for `jekko mcp`.
//!
//! Each test below asserts the gate REJECTS a deliberately-hostile input.
//! This is the synthetic-fake denial coverage required by /test Rule 8:
//! if any of these regress to "silently succeeds", the gate is broken
//! independent of whether the happy path still works.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn jekko() -> Command {
    Command::cargo_bin("jekko").expect("jekko binary")
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 1: shell-metachar args do NOT invoke a shell.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn fixture_shell_metachars_in_target_passed_literally_not_to_shell() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    // Try to attach a "target" containing shell metachars. The string
    // `; rm -rf /tmp/should-not-exist` must be stored LITERALLY as
    // `command`, never executed. We verify by reading the TOML.
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["evil", "; rm -rf /tmp/should-not-exist"])
        .assert()
        .success();
    let body = fs::read_to_string(&cfg).unwrap();
    assert!(body.contains("; rm -rf /tmp/should-not-exist"));
    // The "should-not-exist" path must NOT exist as a side-effect (sanity).
    assert!(!std::path::Path::new("/tmp/should-not-exist").exists());
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 2: status against a runaway (no-newline-flood) server is rejected,
// not OOM'd.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn fixture_status_against_no_newline_flood_rejected_via_line_cap() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    let script = tmp.path().join("flood.py");
    fs::write(
        &script,
        r#"
import sys
# Read the initialize line then flood without a newline.
sys.stdin.readline()
sys.stdout.write("x" * (5 * 1024 * 1024))
sys.stdout.flush()
# Hold open so the client must time out / hit the line cap.
import time
time.sleep(30)
"#,
    )
    .unwrap();
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["flood"])
        .arg("python3")
        .arg("--")
        .arg(&script)
        .assert()
        .success();
    jekko()
        .args(["mcp", "status", "--config"])
        .arg(&cfg)
        .args(["flood", "--timeout", "10"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("protocol violation").or(predicate::str::contains("4194304")),
        );
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 3: malformed JSON in response → ProtocolViolation, not panic.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn fixture_status_against_malformed_json_rejected_as_protocol_violation() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    let script = tmp.path().join("garbage.py");
    fs::write(
        &script,
        r#"
import sys
sys.stdin.readline()
sys.stdout.write("this is not json\n")
sys.stdout.flush()
import time
time.sleep(30)
"#,
    )
    .unwrap();
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["garbage"])
        .arg("python3")
        .arg("--")
        .arg(&script)
        .assert()
        .success();
    jekko()
        .args(["mcp", "status", "--config"])
        .arg(&cfg)
        .args(["garbage", "--timeout", "5"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("protocol violation"));
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 4: invalid name (path traversal, dots, shell metas) rejected at
// attach AND detach.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn fixture_invalid_name_rejected_on_attach_and_detach() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    for bad in ["../../etc/passwd", "$HOME", "name\nwith\nnewlines"] {
        jekko()
            .args(["mcp", "attach", "--config"])
            .arg(&cfg)
            .args([bad, "echo"])
            .assert()
            .failure();
        jekko()
            .args(["mcp", "detach", "--config"])
            .arg(&cfg)
            .args([bad])
            .assert()
            .failure();
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 5: duplicate-name attach without --force is refused; an existing
// good config is NOT clobbered by the failed second attach.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn fixture_duplicate_attach_does_not_clobber_existing_entry() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "ORIGINAL"])
        .assert()
        .success();
    let before = fs::read_to_string(&cfg).unwrap();
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["aara", "REPLACEMENT"])
        .assert()
        .failure();
    let after = fs::read_to_string(&cfg).unwrap();
    assert_eq!(
        before, after,
        "failed duplicate attach must not modify config"
    );
    assert!(after.contains(r#"command = "ORIGINAL""#));
    assert!(!after.contains(r#"command = "REPLACEMENT""#));
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 6: status against a server that returns a JSON-RPC server error
// surfaces that error structurally, not as success.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn fixture_status_surfaces_server_error_not_success() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    let script = tmp.path().join("error_server.py");
    fs::write(
        &script,
        r#"
import json
import sys
line = sys.stdin.readline()
req = json.loads(line)
resp = {"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32601, "message": "Method not found"}}
sys.stdout.write(json.dumps(resp) + "\n")
sys.stdout.flush()
import time
time.sleep(30)
"#,
    )
    .unwrap();
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["errsrv"])
        .arg("python3")
        .arg("--")
        .arg(&script)
        .assert()
        .success();
    jekko()
        .args(["mcp", "status", "--config"])
        .arg(&cfg)
        .args(["errsrv", "--timeout", "5"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("-32601"))
        .stderr(predicate::str::contains("Method not found"));
}
