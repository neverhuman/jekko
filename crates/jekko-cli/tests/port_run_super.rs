//! Integration tests for `jekko port-run --super`.
//!
//! Exercises:
//! - `--dry-run` emits the wave plan for the canonical 12-stage manifest.
//! - The walk-and-mark-complete body persists `complete` for every phase.
//! - `--status <id>` prints persisted phase rows as JSON.
//! - A cyclic manifest is rejected with a "cycle" message on stderr.

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<repo>/crates/jekko-cli`; pop two levels.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

fn canonical_manifest() -> PathBuf {
    repo_root().join("agent/superworkflows/ambitious-superworkflow.superworkflow.json")
}

#[test]
fn dry_run_emits_wave_plan_for_canonical_12_stage() {
    let manifest = canonical_manifest();
    assert!(
        manifest.exists(),
        "canonical 12-stage manifest must exist at {}",
        manifest.display()
    );

    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .arg("port-run")
        .arg("--super")
        .arg(&manifest)
        .arg("--dry-run")
        .output()
        .expect("port-run --dry-run");
    assert!(
        output.status.success(),
        "dry-run failed: status={:?} stderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let value: Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!("dry-run output must be JSON: {err}\n--- stdout ---\n{stdout}")
    });

    let waves = value
        .get("waves")
        .and_then(|v| v.as_array())
        .expect("waves array");
    assert!(
        waves.len() >= 4,
        "expected >= 4 execution waves, got {}",
        waves.len()
    );

    // First wave contains the root phase `source_of_truth`.
    let first_wave: Vec<&str> = waves[0]
        .as_array()
        .expect("first wave array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        first_wave.contains(&"source_of_truth"),
        "first wave must contain `source_of_truth`, got {:?}",
        first_wave
    );

    // Last wave contains the sink phase `final_signoff`.
    let last_wave: Vec<&str> = waves
        .last()
        .and_then(|v| v.as_array())
        .expect("last wave array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        last_wave.contains(&"final_signoff"),
        "last wave must contain `final_signoff`, got {:?}",
        last_wave
    );

    // Every phase appears exactly once across all waves (12 total).
    let mut total = 0usize;
    for wave in waves {
        total += wave.as_array().expect("wave array").len();
    }
    assert_eq!(total, 12, "expected 12 phases total across waves");
}

#[test]
fn mark_complete_walks_all_phases() {
    let manifest = canonical_manifest();
    let tmpdir = tempfile::tempdir().expect("tmpdir");
    let db_path = tmpdir.path().join("supervisor.sqlite");

    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .arg("port-run")
        .arg("--super")
        .arg(&manifest)
        .arg("--db")
        .arg(&db_path)
        .output()
        .expect("port-run live walk");
    assert!(
        output.status.success(),
        "walk failed: status={:?} stderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    // Open the resulting DB and confirm every phase is complete. There is
    // exactly one run row in this tempdir DB.
    let conn = rusqlite::Connection::open(&db_path).expect("open supervisor db");
    let run_id: String = conn
        .query_row("SELECT run_id FROM zyal_super_runs LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("query run id");

    let mut stmt = conn
        .prepare(
            "SELECT phase_id, status FROM zyal_super_phases WHERE run_id = ?1 ORDER BY phase_id",
        )
        .expect("prepare phase query");
    let rows: Vec<(String, String)> = stmt
        .query_map([&run_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query phases")
        .map(|r| r.expect("row"))
        .collect();
    assert_eq!(rows.len(), 12, "expected 12 phase rows, got {}", rows.len());
    for (id, status) in &rows {
        assert_eq!(
            status, "complete",
            "phase `{id}` should be complete, got `{status}`"
        );
    }
}

#[test]
fn status_subcommand_prints_phase_rows() {
    let manifest = canonical_manifest();
    let tmpdir = tempfile::tempdir().expect("tmpdir");
    let db_path = tmpdir.path().join("supervisor.sqlite");

    // Seed the run.
    let mut seed = Command::cargo_bin("jekko").expect("jekko binary");
    let seed_output = seed
        .arg("port-run")
        .arg("--super")
        .arg(&manifest)
        .arg("--db")
        .arg(&db_path)
        .output()
        .expect("seed run");
    assert!(seed_output.status.success(), "seed must succeed");

    // Look up the run id directly from the DB so the test does not depend
    // on stdout parsing.
    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    let run_id: String = conn
        .query_row("SELECT run_id FROM zyal_super_runs LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("query run id");
    drop(conn);

    // Now ask the CLI for status.
    let mut status_cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let status_output = status_cmd
        .arg("port-run")
        .arg("--status")
        .arg(&run_id)
        .arg("--db")
        .arg(&db_path)
        .output()
        .expect("status invocation");
    assert!(
        status_output.status.success(),
        "status failed: stderr=\n{}",
        String::from_utf8_lossy(&status_output.stderr)
    );

    let stdout = String::from_utf8(status_output.stdout).expect("utf8");
    let value: Value =
        serde_json::from_str(&stdout).unwrap_or_else(|err| panic!("status JSON: {err}\n{stdout}"));
    let phases = value
        .get("phases")
        .and_then(|v| v.as_array())
        .expect("phases array");
    assert_eq!(phases.len(), 12, "expected 12 phase rows in status output");

    let expected_ids = [
        "architecture_blueprint",
        "contracts_and_slices",
        "docs_release_ops",
        "final_signoff",
        "hardening_security",
        "integration_fusion",
        "parallel_subsystems",
        "parity_gap_closure",
        "parity_lab",
        "performance_closure",
        "repo_graph_bootstrap",
        "source_of_truth",
    ];
    let seen_ids: Vec<&str> = phases
        .iter()
        .filter_map(|p| p.get("phase_id").and_then(|v| v.as_str()))
        .collect();
    for id in expected_ids.iter() {
        assert!(
            seen_ids.contains(id),
            "status output missing phase id `{id}`; saw {:?}",
            seen_ids
        );
    }
}

#[test]
fn validate_rejects_cycle_manifest() {
    // Build a 9-phase manifest where p00 depends on p08, closing a cycle.
    let mut phase_array: Vec<serde_json::Value> = Vec::new();
    for i in 0..9 {
        let id = format!("p{i:02}");
        let depends_on: Vec<String> = if i == 0 {
            // back-edge: p00 depends on p08 to create a cycle.
            vec!["p08".to_string()]
        } else {
            vec![format!("p{:02}", i - 1)]
        };
        phase_array.push(serde_json::json!({
            "id": id,
            "name": format!("Phase {i}"),
            "objective": "cycle fixture",
            "depends_on": depends_on,
        }));
    }
    let manifest = serde_json::json!({
        "id": "cycle-fixture",
        "name": "Cycle fixture",
        "objective": "Drive the validator to reject a cycle",
        "phases": phase_array,
    });

    let tmpdir = tempfile::tempdir().expect("tmpdir");
    let manifest_path = tmpdir.path().join("cycle.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("encode fixture"),
    )
    .expect("write manifest");

    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .arg("port-run")
        .arg("--super")
        .arg(&manifest_path)
        .arg("--dry-run")
        .output()
        .expect("port-run --dry-run on cycle fixture");
    assert!(
        !output.status.success(),
        "cycle manifest must be rejected; stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        stderr.to_lowercase().contains("cycle"),
        "expected `cycle` in stderr, got:\n{stderr}"
    );

    // Sanity: predicates form of the same check, for the test report.
    let _ = contains("cycle");
}

#[test]
fn live_flag_requires_jekko_zyal_live() {
    // Without `JEKKO_ZYAL_LIVE=1`, `--live` must refuse to run with a clear
    // diagnostic on stderr.
    let manifest = canonical_manifest();
    let tmpdir = tempfile::tempdir().expect("tmpdir");
    let db_path = tmpdir.path().join("supervisor.sqlite");

    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .arg("port-run")
        .arg("--super")
        .arg(&manifest)
        .arg("--db")
        .arg(&db_path)
        .arg("--live")
        .env_remove("JEKKO_ZYAL_LIVE")
        .env_remove("CI")
        .output()
        .expect("port-run --live without opt-in");
    assert!(
        !output.status.success(),
        "--live without JEKKO_ZYAL_LIVE must fail: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        stderr.contains("JEKKO_ZYAL_LIVE"),
        "expected JEKKO_ZYAL_LIVE in stderr, got:\n{stderr}"
    );
}

#[test]
fn live_flag_refuses_ci_env() {
    // With `CI=true`, `--live` must refuse even when JEKKO_ZYAL_LIVE is set.
    let manifest = canonical_manifest();
    let tmpdir = tempfile::tempdir().expect("tmpdir");
    let db_path = tmpdir.path().join("supervisor.sqlite");

    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .arg("port-run")
        .arg("--super")
        .arg(&manifest)
        .arg("--db")
        .arg(&db_path)
        .arg("--live")
        .env("CI", "true")
        .env("JEKKO_ZYAL_LIVE", "1")
        .output()
        .expect("port-run --live under CI=true");
    assert!(
        !output.status.success(),
        "--live with CI=true must fail: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        stderr.contains("CI"),
        "expected `CI` in stderr, got:\n{stderr}"
    );
}

#[test]
fn max_stages_blocks_remaining_phases() {
    // With `--max-stages 3`, only the first three phases reach `complete`;
    // every other phase is recorded `blocked` with the cap reason.
    let manifest = canonical_manifest();
    let tmpdir = tempfile::tempdir().expect("tmpdir");
    let db_path = tmpdir.path().join("supervisor.sqlite");

    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd
        .arg("port-run")
        .arg("--super")
        .arg(&manifest)
        .arg("--db")
        .arg(&db_path)
        .arg("--max-stages")
        .arg("3")
        .output()
        .expect("port-run --max-stages 3");
    assert!(
        output.status.success(),
        "walk failed: status={:?} stderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let conn = rusqlite::Connection::open(&db_path).expect("open supervisor db");
    let run_id: String = conn
        .query_row("SELECT run_id FROM zyal_super_runs LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("query run id");

    let mut stmt = conn
        .prepare(
            "SELECT phase_id, status, summary FROM zyal_super_phases \
             WHERE run_id = ?1 ORDER BY phase_id",
        )
        .expect("prepare phase query");
    let rows: Vec<(String, String, String)> = stmt
        .query_map([&run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query phases")
        .map(|r| r.expect("row"))
        .collect();

    assert_eq!(rows.len(), 12, "expected 12 phase rows, got {}", rows.len());
    let complete_count = rows.iter().filter(|(_, s, _)| s == "complete").count();
    let blocked_count = rows.iter().filter(|(_, s, _)| s == "blocked").count();
    assert_eq!(
        complete_count, 3,
        "expected exactly 3 complete phases with --max-stages 3, got {complete_count}"
    );
    assert_eq!(
        complete_count + blocked_count,
        12,
        "every phase should be complete or blocked, saw {complete_count} complete + {blocked_count} blocked"
    );
    let any_blocked_with_reason = rows
        .iter()
        .filter(|(_, s, _)| s == "blocked")
        .any(|(_, _, summary)| summary.contains("max_stages"));
    assert!(
        any_blocked_with_reason,
        "expected blocked phases to carry `max_stages` summary, got rows: {:?}",
        rows.iter()
            .filter(|(_, s, _)| s == "blocked")
            .collect::<Vec<_>>()
    );
}
