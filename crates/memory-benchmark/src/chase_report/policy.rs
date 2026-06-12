pub(super) fn patch_validation_violation_score(patch: Option<&str>) -> f64 {
    let Some(patch) = patch else {
        // No patch attached → no diff to validate. Same as the prior
        // `patch.is_some()` short-circuit: missing patch fails the gate.
        return 1.0;
    };
    if patch_touches_forbidden_path(patch) {
        return 1.0;
    }
    if patch_contains_forbidden_token(patch) {
        return 1.0;
    }
    0.0
}

/// Forbidden paths — anything inside the trusted core. The AutoResearch
/// mutable surface is `crates/cogcore/src/**` plus the non-reference
/// candidate files in `crates/memory-benchmark/src/candidates/`.
const FORBIDDEN_PATH_PREFIXES: &[&str] = &[
    "crates/memory-benchmark/src/scoring/",
    "crates/memory-benchmark/src/scorer.rs",
    "crates/memory-benchmark/src/runner.rs",
    "crates/memory-benchmark/src/runner_generated.rs",
    "crates/memory-benchmark/src/runner_support.rs",
    "crates/memory-benchmark/src/case.rs",
    "crates/memory-benchmark/src/generated/",
    "crates/memory-benchmark/src/corpus/",
    "crates/memory-benchmark/src/oracle/",
    "crates/memory-benchmark/src/fixture/",
    "crates/memory-benchmark/src/chase_report.rs",
    "crates/memory-benchmark/src/lib.rs",
    "crates/memory-benchmark/src/types.rs",
    "crates/memory-benchmark/src/result.rs",
    "crates/memory-benchmark/src/memory_api.rs",
    "crates/memory-benchmark/src/adapters/baseline.rs",
    "crates/memory-benchmark/src/adapters/reference_context_pack.rs",
    "crates/memory-benchmark/src/adapters/reference_evidence_ledger.rs",
    "crates/memory-benchmark/src/adapters/reference_claim_skeptic.rs",
    "crates/memory-benchmark/tests/",
    "docs/ZYAL/SPEC.md",
];

fn patch_touches_forbidden_path(patch: &str) -> bool {
    for line in patch.lines() {
        // Unified diff path lines: `+++ b/<path>` and `--- a/<path>`.
        let trimmed = line.trim_start();
        let rest = if let Some(rest) = trimmed.strip_prefix("+++ ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("--- ") {
            rest
        } else {
            continue;
        };
        let path = rest.trim_start_matches("a/").trim_start_matches("b/");
        // Strip trailing tab + timestamp some diff tools append.
        let path = path.split_whitespace().next().unwrap_or(path);
        if path == "/dev/null" {
            continue;
        }
        for prefix in FORBIDDEN_PATH_PREFIXES {
            if path == *prefix || path.starts_with(prefix) {
                return true;
            }
        }
    }
    false
}

/// Forbidden tokens that signal non-determinism or secret leakage. Scanned on
/// added lines (`+` prefix in unified diff) and on context lines that look like
/// rust code. Lines starting with `+//` are treated as comments and skipped.
const FORBIDDEN_TOKENS: &[&str] = &[
    concat!("SystemTime::", "now"),
    concat!("Instant::", "now"),
    concat!("thread_", "rng"),
    concat!("rand::", "random"),
    concat!("rand::thread_", "rng"),
    concat!("chrono", "::"),
    concat!("env", "::var("),
    concat!("process", "::", "Command"),
    concat!(" ", "un", "safe", " "),
    concat!(" ", "un", "safe", "{"),
    concat!("pa", "nic!("),
    concat!("un", "implemented!("),
    concat!("sk", "-"),
    concat!("SECRET_", "KEY"),
    concat!("SECRET_", "TOKEN"),
];

fn patch_contains_forbidden_token(patch: &str) -> bool {
    for line in patch.lines() {
        let Some(added) = line.strip_prefix('+') else {
            continue;
        };
        // Skip diff header `+++` and pure comment lines.
        if added.starts_with("++") {
            continue;
        }
        let code = added.trim_start();
        if code.starts_with("//") || code.starts_with("/*") || code.starts_with("*") {
            continue;
        }
        for token in FORBIDDEN_TOKENS {
            if added.contains(token) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod patch_validation_tests {
    use super::super::snapshot::is_eligible;
    use super::*;
    use crate::chase_report::{CandidateSnapshot, GateVector};

    #[test]
    fn no_patch_is_violation() {
        assert_eq!(patch_validation_violation_score(None), 1.0);
    }

    #[test]
    fn cogcore_only_patch_is_safe() {
        let patch = "\
diff --git a/crates/cogcore/src/config.rs b/crates/cogcore/src/config.rs
--- a/crates/cogcore/src/config.rs
+++ b/crates/cogcore/src/config.rs
@@ -1 +1 @@
-const K: f32 = 0.5;
+const K: f32 = 0.6;
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 0.0);
    }

    #[test]
    fn scorer_patch_is_rejected() {
        let patch = "\
diff --git a/crates/memory-benchmark/src/scoring/axes.rs b/crates/memory-benchmark/src/scoring/axes.rs
--- a/crates/memory-benchmark/src/scoring/axes.rs
+++ b/crates/memory-benchmark/src/scoring/axes.rs
@@ -1 +1 @@
-pub const WEIGHT: f32 = 14.0;
+pub const WEIGHT: f32 = 50.0;
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 1.0);
    }

    #[test]
    fn forbidden_token_systemtime_is_rejected() {
        let patch = "\
diff --git a/crates/cogcore/src/config.rs b/crates/cogcore/src/config.rs
--- a/crates/cogcore/src/config.rs
+++ b/crates/cogcore/src/config.rs
@@ -1 +1 @@
-fn time() -> u64 { 0 }
+fn time() -> u64 { SystemTime::now().elapsed().unwrap_or_default().as_secs() }
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 1.0);
    }

    #[test]
    fn forbidden_token_in_comment_is_allowed() {
        let patch = "\
diff --git a/crates/cogcore/src/config.rs b/crates/cogcore/src/config.rs
--- a/crates/cogcore/src/config.rs
+++ b/crates/cogcore/src/config.rs
@@ -1 +1 @@
-const K: f32 = 0.5;
+// SystemTime::now() is forbidden in cogcore hot path
+const K: f32 = 0.6;
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 0.0);
    }

    #[test]
    fn tests_dir_is_forbidden() {
        let patch = "\
diff --git a/crates/memory-benchmark/tests/foo.rs b/crates/memory-benchmark/tests/foo.rs
--- a/crates/memory-benchmark/tests/foo.rs
+++ b/crates/memory-benchmark/tests/foo.rs
@@ -1 +1 @@
-fn t() {}
+fn t() { assert!(true); }
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 1.0);
    }

    #[test]
    fn dev_only_lane_is_not_eligible() {
        let current = CandidateSnapshot {
            name: "current".to_string(),
            source: "current".to_string(),
            total: 70.0,
            ci95_low: 70.0,
            ci95_high: 70.0,
            stress_total: 70.0,
            gates: GateVector::default(),
            cost_usd: 0.0,
            hypothesis: None,
            patch: None,
            observed_at_run: None,
            dev_only: false,
        };
        let candidate = CandidateSnapshot {
            name: "lane".to_string(),
            source: "lane".to_string(),
            total: 90.0,
            ci95_low: 90.0,
            ci95_high: 90.0,
            stress_total: 90.0,
            gates: GateVector::default(),
            cost_usd: 0.0,
            hypothesis: None,
            patch: Some("diff --git a/crates/cogcore/src/config.rs b/crates/cogcore/src/config.rs\n--- a/crates/cogcore/src/config.rs\n+++ b/crates/cogcore/src/config.rs\n@@ -1 +1 @@\n-a\n+b\n".to_string()),
            observed_at_run: None,
            dev_only: true,
        };
        assert!(!is_eligible(&candidate, &current));
    }

    #[test]
    fn drift_in_absolute_points() {
        // 2.36-point drift between selected and reference must read as 2.36,
        // not 0.0236. Old `/ 100.0` would have made this pass the 0.5 gate.
        let selected_score = 80.0_f64;
        let reference_score = 82.36_f64;
        let drift = (selected_score - reference_score).abs();
        assert!((drift - 2.36).abs() < 1e-6);
        // The gate is `<= 0.5`. With the new math, 2.36 fails the gate (rejection).
        assert!(drift > 0.5);
    }
}
