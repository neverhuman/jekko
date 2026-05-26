//! Integration tests for the `jekko watch <run_id>` subcommand (Phase G3).
//!
//! Feeds a small NDJSON event-stream fixture into the per-run path that the
//! watcher tails (`target/zyal/runs/<run_id>/events.jsonl`) and asserts on
//! the rendered output for both `--format json` and `--format plain`.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use predicates::str::contains;

/// Build the per-run events.jsonl path relative to a repo root, mirroring
/// `jankurai_runner::events::run_event_file_rel`.
fn events_path(repo_root: &std::path::Path, run_id: &str) -> PathBuf {
    repo_root
        .join("target")
        .join("zyal")
        .join("runs")
        .join(run_id)
        .join("events.jsonl")
}

/// Seed the events file with the given JSON lines.
fn seed_events(repo_root: &std::path::Path, run_id: &str, lines: &[serde_json::Value]) {
    let path = events_path(repo_root, run_id);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut buf = String::new();
    for line in lines {
        buf.push_str(&serde_json::to_string(line).unwrap());
        buf.push('\n');
    }
    fs::write(&path, buf).unwrap();
}

#[test]
fn watch_once_emits_json_snapshot_for_fixture_stream() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "g3-json-fixture";
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap();
    seed_events(
        tmp.path(),
        run_id,
        &[
            serde_json::json!({
                "ts": now - 10,
                "kind": "run_started",
                "run_id": run_id,
                "data": {"pool_size": 4}
            }),
            serde_json::json!({
                "ts": now - 9,
                "kind": "reasoning_lane",
                "run_id": run_id,
                "data": {"id": "lane-1", "status": "started"}
            }),
            serde_json::json!({
                "ts": now - 8,
                "kind": "reasoning_lane",
                "run_id": run_id,
                "data": {"id": "lane-1", "status": "complete"}
            }),
            serde_json::json!({
                "ts": now - 7,
                "kind": "audit_result",
                "run_id": run_id,
                "data": {"score": 92, "hard_findings": 0}
            }),
        ],
    );

    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .args(["watch", run_id, "--once", "--format", "json", "--repo-root"])
        .arg(tmp.path())
        .output()
        .expect("watch invocation");
    assert!(
        output.status.success(),
        "watch failed: status={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("watch json output should parse");
    let snap = &parsed["snapshot"];
    assert_eq!(snap["lanes_started"].as_u64(), Some(1));
    assert_eq!(snap["lanes_finished"].as_u64(), Some(1));
    assert_eq!(snap["last_jankurai_score"].as_i64(), Some(92));
    assert_eq!(snap["last_jankurai_hard_findings"].as_i64(), Some(0));
    assert!(parsed.get("actions").is_some());
}

#[test]
fn watch_plain_includes_remediation_summary_on_stall() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "g3-plain-stall";
    // Stale last_progress_ts — 10_000s ago, well past the 60s threshold we
    // pass to the CLI. The watcher should fire `stall_detected`.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap();
    let stale_ts = now.saturating_sub(10_000);
    seed_events(
        tmp.path(),
        run_id,
        &[
            serde_json::json!({
                "ts": stale_ts,
                "kind": "run_started",
                "run_id": run_id,
                "data": {"pool_size": 4}
            }),
            serde_json::json!({
                "ts": stale_ts + 1,
                "kind": "worker_started",
                "run_id": run_id,
                "data": {"worker": "w-01"}
            }),
        ],
    );

    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    cmd.args([
        "watch",
        run_id,
        "--once",
        "--format",
        "plain",
        "--stall-threshold",
        "60",
        "--repo-root",
    ])
    .arg(tmp.path())
    .assert()
    .success()
    .stdout(contains("stall_detected"))
    .stdout(contains("worker_started"));
}

#[test]
fn watch_no_follow_short_circuits_after_drain() {
    // Sanity check: --no-follow exits the same way --once does, without
    // touching the notify watcher.
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "g3-no-follow";
    seed_events(
        tmp.path(),
        run_id,
        &[serde_json::json!({
            "ts": 1,
            "kind": "run_started",
            "run_id": run_id,
            "data": {}
        })],
    );
    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    cmd.args([
        "watch",
        run_id,
        "--no-follow",
        "--format",
        "plain",
        "--repo-root",
    ])
    .arg(tmp.path())
    .assert()
    .success()
    .stdout(contains("run_started"));
}

#[test]
fn watch_tui_renders_snapshot_to_test_backend() {
    // Phase G2: `--format tui --tui-once-snapshot` should render a real
    // Ratatui dashboard into a `TestBackend` and dump the rendered text to
    // stdout. The output must include each pane title from the spec.
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "g2-tui-snapshot";
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap();
    seed_events(
        tmp.path(),
        run_id,
        &[
            serde_json::json!({
                "ts": now - 5,
                "kind": "run_started",
                "run_id": run_id,
                "data": {"pool_size": 4}
            }),
            serde_json::json!({
                "ts": now - 4,
                "kind": "reasoning_lane",
                "run_id": run_id,
                "data": {"id": "lane-1", "status": "started"}
            }),
            serde_json::json!({
                "ts": now - 3,
                "kind": "reasoning_lane",
                "run_id": run_id,
                "data": {"id": "lane-1", "status": "complete"}
            }),
            serde_json::json!({
                "ts": now - 2,
                "kind": "audit_result",
                "run_id": run_id,
                "data": {"score": 93, "hard_findings": 0}
            }),
        ],
    );
    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .args([
            "watch",
            run_id,
            "--format",
            "tui",
            "--tui-once-snapshot",
            "--repo-root",
        ])
        .arg(tmp.path())
        .output()
        .expect("watch invocation");
    assert!(
        output.status.success(),
        "watch tui snapshot failed: status={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each pane title must appear, plus the run id and Jankurai score.
    for needle in [
        "Lanes",
        "Parity",
        "Model",
        "Active rules",
        "Jankurai",
        "ZYAL Watcher",
        run_id,
    ] {
        assert!(
            stdout.contains(needle),
            "tui snapshot missing {needle:?}; got:\n{stdout}"
        );
    }
}

#[test]
fn watch_tui_once_snapshot_works_without_existing_events_file() {
    // Empty / missing event stream should still render a sensible frame
    // (zeros across the board, no panic) and exit 0.
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .args([
            "watch",
            "g2-empty",
            "--format",
            "tui",
            "--tui-once-snapshot",
            "--repo-root",
        ])
        .arg(tmp.path())
        .output()
        .expect("watch invocation");
    assert!(
        output.status.success(),
        "watch tui (empty) failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Lanes"), "missing Lanes pane: {stdout}");
    assert!(
        stdout.contains("none firing"),
        "expected empty rules placeholder, got: {stdout}"
    );
}
