use std::fs;
use std::path::Path;

use super::emit::{emit_superworkflow, emit_toml, strip_pragmas};
use super::target::source_reference;
use super::*;

#[test]
fn round_trip_strips_pragmas() {
    let raw = "# zyal: declarative target=toml schema=test@1\nschema_version: \"1.0.0\"\nlanes:\n  - name: a\n    command_id: x.a\n    cost: 1\n";
    let stripped = strip_pragmas(raw);
    assert!(!stripped.contains("# zyal:"));
    assert!(stripped.contains("schema_version"));
}

#[test]
fn toml_emit_basic() {
    let raw = "# zyal: declarative target=toml schema=t@1\nschema_version: \"1.0.0\"\nlanes:\n  - name: a\n    cost: 1\n";
    let out = emit_toml(raw).expect("emit");
    assert!(out.contains("schema_version"));
    assert!(out.contains("[[lane]]"));
    assert!(out.contains("name = \"a\""));
}

#[test]
fn idempotent_emit() {
    let raw = "# zyal: declarative target=toml schema=t@1\nschema_version: \"1.0.0\"\nlanes:\n  - name: a\n    cost: 1\n";
    let a = emit_toml(raw).unwrap();
    let b = emit_toml(raw).unwrap();
    assert_eq!(a, b, "compile must be idempotent");
}

#[test]
fn runbook_profiles_validate_without_emitting_legacy_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("smoke.zyal");
    fs::write(
        &source,
        "<<<ZYAL v1:daemon id=smoke>>>\njob:\n  name: smoke\n<<<END_ZYAL id=smoke>>>\n",
    )
    .unwrap();

    let outcome = compile_one(&source, None, true).unwrap();
    assert!(matches!(outcome, Outcome::Unchanged(path) if path == source));
    assert!(
        !source.with_extension("yml").exists(),
        "runbook validation must not emit retired .zyal.yml artifacts"
    );
}

#[test]
fn source_reference_preserves_canonical_subdirectory() {
    assert_eq!(
        source_reference(Path::new("./agent/zyal/sandbox-lanes.zyal")),
        "agent/zyal/sandbox-lanes.zyal"
    );
}

// --- SuperWorkflow validation + emission ----------------------------------

/// Build a minimal valid SuperWorkflow manifest with `n` independent phases
/// (`p0`, `p1`, ...). Used by the validation tests below to exercise edge
/// cases without repeating the entire pragma/header preamble.
fn superworkflow_with_phases(n: usize) -> String {
    let mut raw = String::from(
        "# zyal: declarative target=superworkflow schema=zyal/superworkflow@1\n\
         version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: smoke\n\
         job:\n  name: smoke\n  objective: smoke\n\
         superworkflow:\n",
    );
    raw.push_str(&format!("  stage_count: {n}\n  phases:\n"));
    for idx in 0..n {
        raw.push_str(&format!(
            "    - id: p{idx}\n      name: p{idx}\n      objective: p{idx}\n      exit:\n        required_artifacts: [target/p{idx}.json]\n        gates:\n          - kind: artifact_exists\n"
        ));
    }
    raw
}

#[test]
fn superworkflow_emit_requires_nine_to_twelve_phases() {
    let raw = superworkflow_with_phases(9);
    let out = emit_superworkflow(&raw).expect("9-phase superworkflow json");
    assert!(out.contains("\"superworkflow\""));
    assert!(out.contains("\"phases\""));

    let raw = superworkflow_with_phases(12);
    emit_superworkflow(&raw).expect("12-phase superworkflow json");
}

#[test]
fn superworkflow_rejects_too_few_phases() {
    let raw = superworkflow_with_phases(1);
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(format!("{err}").contains("requires 9-12 phases"));
}

#[test]
fn superworkflow_rejects_duplicate_phase_ids() {
    // Rewrite the `p1` block so it claims `id: p0`, colliding with the first.
    let raw = superworkflow_with_phases(9).replace("- id: p1\n", "- id: p0\n");
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("duplicate superworkflow phase id"),
        "expected duplicate id error, got: {err}"
    );
}

#[test]
fn superworkflow_rejects_self_dependency() {
    let mut raw = superworkflow_with_phases(9);
    raw = raw.replacen(
        "- id: p0\n      name: p0\n      objective: p0\n      exit:\n",
        "- id: p0\n      name: p0\n      objective: p0\n      depends_on: [p0]\n      exit:\n",
        1,
    );
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("depends on itself"),
        "expected self-dependency error, got: {err}"
    );
}

#[test]
fn superworkflow_rejects_unknown_dependency() {
    let mut raw = superworkflow_with_phases(9);
    raw = raw.replacen(
        "- id: p0\n      name: p0\n      objective: p0\n      exit:\n",
        "- id: p0\n      name: p0\n      objective: p0\n      depends_on: [nope]\n      exit:\n",
        1,
    );
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("unknown phase"),
        "expected unknown-dependency error, got: {err}"
    );
}

#[test]
fn superworkflow_rejects_cycle() {
    // Wire `p0` -> `p1` and `p1` -> `p0` so the dependency graph is a 2-cycle.
    let mut raw = superworkflow_with_phases(9);
    raw = raw.replacen(
        "- id: p0\n      name: p0\n      objective: p0\n      exit:\n",
        "- id: p0\n      name: p0\n      objective: p0\n      depends_on: [p1]\n      exit:\n",
        1,
    );
    raw = raw.replacen(
        "- id: p1\n      name: p1\n      objective: p1\n      exit:\n",
        "- id: p1\n      name: p1\n      objective: p1\n      depends_on: [p0]\n      exit:\n",
        1,
    );
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("cycle"),
        "expected cycle error, got: {err}"
    );
}
