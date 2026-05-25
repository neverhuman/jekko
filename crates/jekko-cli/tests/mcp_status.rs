//! `jekko mcp status` integration tests.
//!
//! Drives the full attach → spawn → initialize → tools/list flow against a
//! minimal Python MCP-style echo server inlined here. No external network
//! access. The fixtures verify the JSON-RPC handshake end-to-end, including
//! `initialized` notification and `tools/list` ToolDescriptor parsing.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn jekko() -> Command {
    Command::cargo_bin("jekko").expect("jekko binary")
}

fn write_echo_server(dir: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("echo_mcp_server.py");
    fs::write(
        &script,
        r#"
import json
import sys


def write(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def main():
    # initialize
    line = sys.stdin.readline()
    if not line:
        return
    req = json.loads(line)
    if req.get("method") != "initialize":
        write({"jsonrpc": "2.0", "id": req["id"],
               "error": {"code": -32601, "message": "expected initialize"}})
        return
    write({
        "jsonrpc": "2.0",
        "id": req["id"],
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "echo-test-server", "version": "0.1.0"},
        },
    })
    # initialized notification (no response)
    notif = sys.stdin.readline()
    _ = json.loads(notif)
    # tools/list
    line = sys.stdin.readline()
    req = json.loads(line)
    if req.get("method") != "tools/list":
        write({"jsonrpc": "2.0", "id": req["id"],
               "error": {"code": -32601, "message": "expected tools/list"}})
        return
    write({
        "jsonrpc": "2.0",
        "id": req["id"],
        "result": {"tools": [
            {"name": "echo_one", "description": "echo a single string"},
            {"name": "echo_two"},  # no description allowed
        ]},
    })


if __name__ == "__main__":
    main()
"#,
    )
    .unwrap();
    script
}

#[test]
fn status_against_echo_server_returns_ok_and_tools() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    let script = write_echo_server(tmp.path());

    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["echo"])
        .arg("python3")
        .arg("--")
        .arg(&script)
        .assert()
        .success();

    jekko()
        .args(["mcp", "status", "--config"])
        .arg(&cfg)
        .args(["echo", "--timeout", "10"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mcp server `echo`: OK"))
        .stdout(predicate::str::contains("server: echo-test-server"))
        .stdout(predicate::str::contains("protocol: 2024-11-05"))
        .stdout(predicate::str::contains("tools: 2"))
        .stdout(predicate::str::contains("echo_one"))
        .stdout(predicate::str::contains("echo_two"));
}

#[test]
fn status_unknown_server_errors() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    fs::write(&cfg, "").unwrap();
    jekko()
        .args(["mcp", "status", "--config"])
        .arg(&cfg)
        .args(["ghost"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found in config"));
}

#[test]
fn status_against_crashy_server_surfaces_early_exit() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("mcp.toml");
    let script = tmp.path().join("crashy.py");
    fs::write(
        &script,
        r#"
import sys
print("ERROR: simulated boot failure", file=sys.stderr, flush=True)
sys.exit(2)
"#,
    )
    .unwrap();
    jekko()
        .args(["mcp", "attach", "--config"])
        .arg(&cfg)
        .args(["crashy"])
        .arg("python3")
        .arg("--")
        .arg(&script)
        .assert()
        .success();

    jekko()
        .args(["mcp", "status", "--config"])
        .arg(&cfg)
        .args(["crashy", "--timeout", "5"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("exited before responding"))
        .stderr(predicate::str::contains("simulated boot failure"));
}
