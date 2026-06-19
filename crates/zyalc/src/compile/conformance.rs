//! Conformance suite (M17) — locks the ZYAL contract.
//!
//! A table of fixtures asserts that (1) every valid doc compiles clean, (2) every
//! invalid doc fails with its DECLARED diagnostic code, and (3) the IR is
//! byte-deterministic. This is the determinism + diagnostic CI gate — it runs on
//! `cargo test` and is exposed to the CLI via [`run_conformance`]. Seeding the
//! fixtures inline (rather than on disk) keeps the gate self-contained.

#![allow(dead_code)] // conformance helpers are called from compile::run_conformance (M17-cont CLI) + unit tests

const HEADER: &str = "# zyal: declarative target=flowgraph schema=zyal/flowgraph@1\n\
     version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: cf\n\
     job:\n  name: cf\n  objective: conformance\n";

/// What a fixture must do.
pub(super) enum Expect {
    /// Compiles clean.
    Valid,
    /// Fails with a diagnostic whose message contains this code/marker.
    Invalid(&'static str),
}

pub(super) struct Case {
    pub name: &'static str,
    pub source: String,
    pub expect: Expect,
    pub source_path: Option<&'static str>,
    pub files: Vec<(&'static str, &'static str)>,
}

fn valid(name: &'static str, body: &str) -> Case {
    Case {
        name,
        source: format!("{HEADER}{body}"),
        expect: Expect::Valid,
        source_path: None,
        files: Vec::new(),
    }
}
fn invalid(name: &'static str, body: &str, code: &'static str) -> Case {
    Case {
        name,
        source: format!("{HEADER}{body}"),
        expect: Expect::Invalid(code),
        source_path: None,
        files: Vec::new(),
    }
}
fn invalid_with_files(
    name: &'static str,
    body: &str,
    code: &'static str,
    files: Vec<(&'static str, &'static str)>,
) -> Case {
    Case {
        name,
        source: format!("{HEADER}{body}"),
        expect: Expect::Invalid(code),
        source_path: Some("agent/zyal/main.zyal"),
        files,
    }
}

/// The seeded conformance fixtures (one per diagnostic + representative valids).
pub(super) fn cases() -> Vec<Case> {
    vec![
        // ---- valid -----------------------------------------------------------
        valid("minimal", "graph:\n  nodes:\n    - { id: a, type: agent, label: A }\n"),
        valid(
            "routing_chain",
            "graph:\n  nodes:\n    - { id: route, type: router, router: { drafts: 3, fusion: 1 } }\n",
        ),
        valid(
            "patterns_bounded",
            "patterns:\n  sweep:\n    kind: scatter_gather\n    split: { count: 3 }\n\
             \x20 refine:\n    kind: loop_until_dry\n    stop: { max_rounds: 8 }\n",
        ),
        valid(
            "tournament",
            "agents:\n  supervisor:\n    agent: plan\n\
             hero_judge:\n  generations: 3\n  population:\n    hero_lanes: 4\n",
        ),
        valid(
            "typed_ports_ok",
            "graph:\n  nodes:\n\
             \x20   - { id: a, type: agent, port_types: { out: { result: string } } }\n\
             \x20   - { id: b, type: agent, port_types: { in: { prompt: string } } }\n\
             \x20 edges:\n    - { from: a.result, to: b.prompt }\n",
        ),
        valid(
            "bounded_discovered",
            "graph:\n  nodes:\n    - { id: g, type: fanout, cardinality: { mode: discovered, max: 16, identity: url } }\n",
        ),
        valid(
            "contract_header_ok",
            "zyal:\n  version: \"1.0\"\n  contract: { registry_version: \"1.0.0\", ir_version: \"flowgraph.v3\" }\n\
             graph:\n  nodes:\n    - { id: a, type: agent, label: A }\n",
        ),
        valid(
            "gated_host_action",
            "graph:\n  nodes:\n\
             \x20   - { id: a, type: agent }\n    - { id: g, type: budget_gate }\n    - { id: t, type: tool }\n\
             \x20 edges:\n    - { from: a, to: g }\n    - { from: g, to: t }\n",
        ),
        // ---- invalid (each asserts its diagnostic) ---------------------------
        invalid("unknown_kind", "graph:\n  nodes:\n    - { id: x, type: not_a_kind }\n", "ZYAL_E_UNKNOWN_NODE_KIND"),
        invalid(
            "edge_type_mismatch",
            "types: { Draft: string, Page: number }\n\
             graph:\n  nodes:\n\
             \x20   - { id: a, type: agent, port_types: { out: { result: Page } } }\n\
             \x20   - { id: b, type: agent, port_types: { in: { prompt: Draft } } }\n\
             \x20 edges:\n    - { from: a.result, to: b.prompt }\n",
            "ZYAL_E_EDGE_TYPE_MISMATCH",
        ),
        invalid(
            "taint_violation",
            "graph:\n  nodes:\n\
             \x20   - { id: src, type: data_feed }\n    - { id: m, type: agent }\n\
             \x20 edges:\n    - { from: src, to: m, taint: [secret] }\n",
            "ZYAL_E_TAINT_VIOLATION",
        ),
        invalid(
            "unbounded_cardinality",
            "graph:\n  nodes:\n    - { id: pool, type: fanout, cardinality: { mode: discovered, identity: url } }\n",
            "ZYAL_E_DYNAMIC_CARDINALITY_UNBOUNDED",
        ),
        invalid(
            "no_identity",
            "graph:\n  nodes:\n    - { id: pool, type: fanout, cardinality: { mode: discovered, max: 16 } }\n",
            "ZYAL_E_DYNAMIC_CARDINALITY_NO_IDENTITY",
        ),
        invalid(
            "unbounded_scatter",
            "patterns:\n  s:\n    kind: scatter_gather\n    branch: { agent: w }\n",
            "ZYAL_E_PATTERN_UNBOUNDED_SCATTER",
        ),
        invalid(
            "unbounded_loop",
            "patterns:\n  l:\n    kind: loop_until_dry\n    stop: { marginal_utility_below: 0.05 }\n",
            "ZYAL_E_PATTERN_UNBOUNDED_LOOP",
        ),
        invalid("dotted_id", "graph:\n  nodes:\n    - { id: a.b, type: agent }\n", "must not contain"),
        invalid(
            "duplicate_id",
            "graph:\n  nodes:\n    - { id: dup, type: agent }\n    - { id: dup, type: agent }\n",
            "duplicate",
        ),
        // ---- M4-cont: authority-as-topology ----------------------------------
        invalid(
            "retry_without_idempotency",
            "graph:\n  nodes:\n    - { id: w, type: artifact, retry: { max: 3 } }\n",
            "ZYAL_E_RETRY_WITHOUT_IDEMPOTENCY",
        ),
        invalid(
            "effect_needs_approval",
            "graph:\n  nodes:\n\
             \x20   - { id: a, type: agent }\n    - { id: t, type: tool }\n\
             \x20 edges:\n    - { from: a, to: t }\n",
            "ZYAL_E_EFFECT_NEEDS_APPROVAL",
        ),
        invalid(
            "capability_denied",
            "graph:\n  nodes:\n    - { id: x, type: agent, capabilities: { requires: [credential.export] } }\n",
            "ZYAL_E_CAPABILITY_DENIED",
        ),
        // ---- M4-cont: zyal: contract header ----------------------------------
        invalid("version_missing", "zyal:\n  dialect: flowgraph\n", "ZYAL_E_VERSION_MISSING"),
        invalid("unsupported_major", "zyal:\n  version: \"3.0\"\n", "ZYAL_E_UNSUPPORTED_MAJOR"),
        invalid(
            "registry_too_old",
            "zyal:\n  version: \"1.0\"\n  contract: { registry_version: \"9.0.0\" }\n",
            "ZYAL_E_REGISTRY_TOO_OLD",
        ),
        invalid(
            "ir_too_new",
            "zyal:\n  version: \"1.0\"\n  contract: { ir_version: \"flowgraph.v9\" }\n",
            "ZYAL_E_IR_TOO_NEW_FOR_RUNNER",
        ),
        // ---- uses/library diagnostics ----------------------------------------
        invalid("uses_shape", "uses: nope\n", "ZYAL_E_USES_SHAPE"),
        invalid(
            "unsupported_ref",
            "uses:\n  - { ref: http://example.invalid/lib, as: lib }\n",
            "ZYAL_E_UNSUPPORTED_REF",
        ),
        invalid(
            "use_alias_conflict",
            "uses:\n  - { ref: zyal://global/a@1, as: lib }\n  - { ref: zyal://global/b@1, as: lib }\n",
            "ZYAL_E_USE_ALIAS_CONFLICT",
        ),
        invalid_with_files(
            "missing_ref",
            "uses:\n  - { ref: zyal://global/nope@1, as: nope }\n",
            "ZYAL_E_MISSING_REF",
            Vec::new(),
        ),
        invalid_with_files(
            "ref_cycle",
            "uses:\n  - { ref: zyal://local/a@1, as: a }\n",
            "ZYAL_E_REF_CYCLE",
            vec![
                (
                    "agent/zyal/a.zyal",
                    "exports:\n  - ref: zyal://local/a@1\n    interface: { inputs: [], outputs: [] }\nuses:\n  - { ref: zyal://local/b@1, as: b }\nid: a\njob: { name: a, objective: a }\n",
                ),
                (
                    "agent/zyal/b.zyal",
                    "exports:\n  - ref: zyal://local/b@1\n    interface: { inputs: [], outputs: [] }\nuses:\n  - { ref: zyal://local/a@1, as: a }\nid: b\njob: { name: b, objective: b }\n",
                ),
            ],
        ),
    ]
}

/// The outcome of running the conformance suite.
#[derive(Debug, Default)]
pub(super) struct ConformanceReport {
    pub passed: usize,
    /// `(case_name, what_went_wrong)` for each failure.
    pub failures: Vec<(&'static str, String)>,
}

impl ConformanceReport {
    pub fn ok(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Run every fixture, checking expected outcomes AND byte-determinism on valids.
pub(super) fn run_conformance() -> ConformanceReport {
    let mut report = ConformanceReport::default();
    for case in cases() {
        let source_path = prepare_case_files(&case);
        let first_result = match source_path.as_deref() {
            Some(path) => super::emit::emit_flowgraph_with_source(&case.source, Some(path)),
            None => super::emit::emit_flowgraph(&case.source),
        };
        match (&case.expect, first_result) {
            (Expect::Valid, Ok(first)) => {
                // determinism: a second compile must be byte-identical
                let second_result = match source_path.as_deref() {
                    Some(path) => super::emit::emit_flowgraph_with_source(&case.source, Some(path)),
                    None => super::emit::emit_flowgraph(&case.source),
                };
                match second_result {
                    Ok(second) if second == first => report.passed += 1,
                    Ok(_) => report
                        .failures
                        .push((case.name, "non-deterministic: recompile differs".into())),
                    Err(e) => report
                        .failures
                        .push((case.name, format!("recompile failed: {e}"))),
                }
            }
            (Expect::Valid, Err(e)) => report
                .failures
                .push((case.name, format!("expected valid, failed: {e}"))),
            (Expect::Invalid(code), Err(e)) => {
                if format!("{e}").contains(code) {
                    report.passed += 1;
                } else {
                    report
                        .failures
                        .push((case.name, format!("expected `{code}`, got `{e}`")));
                }
            }
            (Expect::Invalid(code), Ok(_)) => report
                .failures
                .push((case.name, format!("expected `{code}`, compiled clean"))),
        }
    }
    report
}

fn prepare_case_files(case: &Case) -> Option<std::path::PathBuf> {
    let source_rel = case.source_path?;
    let root = std::env::temp_dir()
        .join("zyalc-conformance")
        .join(std::process::id().to_string())
        .join(case.name);
    let _ = std::fs::remove_dir_all(&root);
    for (rel, body) in &case.files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir conformance fixture parent");
        }
        std::fs::write(path, body).expect("write conformance fixture");
    }
    let source_path = root.join(source_rel);
    if let Some(parent) = source_path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir conformance source parent");
    }
    Some(source_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conformance_suite_is_green() {
        let report = run_conformance();
        assert!(
            report.ok(),
            "conformance failures ({} passed):\n{}",
            report.passed,
            report
                .failures
                .iter()
                .map(|(n, why)| format!("  - {n}: {why}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        // sanity: the suite actually exercised every fixture
        assert_eq!(report.passed, cases().len());
    }

    #[test]
    fn every_diagnostic_code_is_covered() {
        // guard against silently dropping a diagnostic from the gate
        let codes: Vec<&str> = cases()
            .iter()
            .filter_map(|c| match c.expect {
                Expect::Invalid(code) => Some(code),
                Expect::Valid => None,
            })
            .collect();
        for required in [
            "ZYAL_E_UNKNOWN_NODE_KIND",
            "ZYAL_E_EDGE_TYPE_MISMATCH",
            "ZYAL_E_TAINT_VIOLATION",
            "ZYAL_E_DYNAMIC_CARDINALITY_UNBOUNDED",
            "ZYAL_E_DYNAMIC_CARDINALITY_NO_IDENTITY",
            "ZYAL_E_PATTERN_UNBOUNDED_SCATTER",
            "ZYAL_E_PATTERN_UNBOUNDED_LOOP",
            "ZYAL_E_RETRY_WITHOUT_IDEMPOTENCY",
            "ZYAL_E_EFFECT_NEEDS_APPROVAL",
            "ZYAL_E_CAPABILITY_DENIED",
            "ZYAL_E_VERSION_MISSING",
            "ZYAL_E_UNSUPPORTED_MAJOR",
            "ZYAL_E_REGISTRY_TOO_OLD",
            "ZYAL_E_IR_TOO_NEW_FOR_RUNNER",
            "ZYAL_E_USES_SHAPE",
            "ZYAL_E_UNSUPPORTED_REF",
            "ZYAL_E_USE_ALIAS_CONFLICT",
            "ZYAL_E_MISSING_REF",
            "ZYAL_E_REF_CYCLE",
        ] {
            assert!(
                codes.contains(&required),
                "conformance suite is missing `{required}`"
            );
        }
    }

    /// Self-maintaining gate (M17-cont): scan the compiler's validator sources for
    /// EVERY `ZYAL_E_*` diagnostic they emit and assert each has a conformance
    /// fixture. Adding a new diagnostic without a fixture fails CI — the gate can
    /// no longer silently fall behind the compiler.
    #[test]
    fn every_emitted_diagnostic_has_a_fixture() {
        use std::collections::BTreeSet;
        let covered: BTreeSet<&str> = cases()
            .iter()
            .filter_map(|c| match c.expect {
                Expect::Invalid(code) if code.starts_with("ZYAL_E_") => Some(code),
                _ => None,
            })
            .collect();
        let dir = format!("{}/src/compile", env!("CARGO_MANIFEST_DIR"));
        let mut emitted: BTreeSet<String> = BTreeSet::new();
        for entry in std::fs::read_dir(&dir).expect("read src/compile") {
            let path = entry.expect("dir entry").path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Scan the VALIDATORS that emit diagnostics — not the fixtures
            // (conformance.rs) nor the test file (which only asserts on codes).
            if !name.ends_with(".rs") || name == "conformance.rs" || name == "tests.rs" {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap_or_default();
            for tok in extract_diagnostics(&src) {
                emitted.insert(tok);
            }
        }
        let missing: Vec<&String> = emitted
            .iter()
            .filter(|d| !covered.contains(d.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "diagnostics emitted by the compiler but missing a conformance fixture: {missing:?}"
        );
    }

    /// Extract every `ZYAL_E_<UPPER_SNAKE>` token from a source string.
    fn extract_diagnostics(src: &str) -> Vec<String> {
        let bytes = src.as_bytes();
        let needle = b"ZYAL_E_";
        let mut out = Vec::new();
        let mut i = 0;
        while i + needle.len() <= bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                let mut j = i + needle.len();
                while j < bytes.len() && (bytes[j].is_ascii_uppercase() || bytes[j] == b'_') {
                    j += 1;
                }
                out.push(String::from_utf8_lossy(&bytes[i..j]).to_string());
                i = j;
            } else {
                i += 1;
            }
        }
        out
    }
}
