//! Target-switched parity report schema and checker.
//!
//! Split into focused submodules. The public surface is preserved via
//! re-exports below, so callers continue to use `crate::parity_lab::*`.

mod adapters;
mod artifacts;
mod checker;
mod gaps;
mod helpers;
mod runner;
mod types;

pub use adapters::{CommandTargetAdapter, FakeTargetAdapter, TargetAdapter};
pub use artifacts::{generated_manifest, summarize_report, write_report_artifacts};
pub use checker::{check_report, check_summary_artifact};
pub use gaps::{generate_gaps, parity_gap_to_followup_task};
pub use runner::{approved_cases, load_cases_from_dir, run_cases, run_target_switched_cases};
pub use types::{
    GeneratedParityCase, GeneratedParityManifest, ParityArtifacts, ParityCase, ParityGap,
    ParityPerfBudget, ParityReport, ParityResult, ParityStep, ParitySummary, RawParityRow,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn required_case(id: &str) -> ParityCase {
        ParityCase {
            id: id.into(),
            tags: vec!["required".into(), "approved".into()],
            target_kind: "fake".into(),
            steps: vec![ParityStep {
                send: "PING".into(),
                expect: "PONG".into(),
            }],
            perf: None,
        }
    }

    #[test]
    fn checker_accepts_required_pass() {
        let cases = vec![required_case("ping")];
        let report = ParityReport {
            schema_version: "zyal.parity.v1".into(),
            reference: "ref".into(),
            candidate: "cand".into(),
            results: vec![ParityResult {
                case_id: "ping".into(),
                target: "cand".into(),
                status: "passed".into(),
                skipped: false,
                message: None,
                perf: None,
                ..ParityResult::default()
            }],
        };
        check_report(&cases, &report).unwrap();
    }

    #[test]
    fn checker_rejects_missing_skipped_failed_and_perfless_cases() {
        let mut perf_case = required_case("perf");
        perf_case.perf = Some(ParityPerfBudget {
            p95_ms_max_ratio: Some(1.25),
        });
        let cases = vec![
            required_case("missing"),
            required_case("skip"),
            required_case("fail"),
            perf_case,
        ];
        let report = ParityReport {
            schema_version: "zyal.parity.v1".into(),
            reference: "ref".into(),
            candidate: "cand".into(),
            results: vec![
                ParityResult {
                    case_id: "skip".into(),
                    target: "cand".into(),
                    status: "passed".into(),
                    skipped: true,
                    message: None,
                    perf: None,
                    ..ParityResult::default()
                },
                ParityResult {
                    case_id: "fail".into(),
                    target: "cand".into(),
                    status: "failed".into(),
                    skipped: false,
                    message: Some("no".into()),
                    perf: None,
                    ..ParityResult::default()
                },
                ParityResult {
                    case_id: "perf".into(),
                    target: "cand".into(),
                    status: "passed".into(),
                    skipped: false,
                    message: None,
                    perf: None,
                    ..ParityResult::default()
                },
            ],
        };
        let err = check_report(&cases, &report).unwrap_err().to_string();
        assert!(err.contains("missing"));
        assert!(err.contains("skipped"));
        assert!(err.contains("failed"));
        assert!(err.contains("missing perf"));
    }

    #[test]
    fn fake_adapter_produces_checkable_report() {
        let cases = vec![required_case("ping")];
        let mut adapter = FakeTargetAdapter::new("candidate");
        let report = run_cases(&mut adapter, &cases, "reference", "candidate").unwrap();
        check_report(&cases, &report).unwrap();
    }

    #[test]
    fn command_adapter_passes_and_fails_cases() {
        let mut pass = required_case("echo-pass");
        pass.steps[0].expect = "PING".into();
        let mut adapter = CommandTargetAdapter::new("candidate", "cat", ".");
        let result = adapter.run_case(&pass).unwrap();
        assert_eq!(result.status, "passed");

        let mut fail = required_case("echo-fail");
        fail.steps[0].expect = "NOPE".into();
        let result = adapter.run_case(&fail).unwrap();
        assert_eq!(result.status, "failed");
    }

    #[test]
    fn command_oracle_blocks_gate_when_candidate_disagrees() {
        // Real reference/candidate switch: the reference echoes stdin so it
        // matches the expected text; the candidate prints a fixed wrong value.
        // The summary must be non-passing with gaps, so the orchestrator's
        // `parity_gate_passed` would be false — i.e. the gate is now truthful
        // rather than always-pass (the U2 fix).
        let mut case = required_case("switch");
        case.steps[0].send = "PONG".into();
        case.steps[0].expect = "PONG".into();
        let mut reference = CommandTargetAdapter::new("reference", "cat", ".");
        let mut candidate = CommandTargetAdapter::new("candidate", "printf NOPE", ".");
        let report =
            run_target_switched_cases(&mut reference, &mut candidate, &[case.clone()]).unwrap();
        let summary = summarize_report(&[case], report);
        assert_ne!(summary.status, "passed");
        assert!(!summary.gaps.is_empty());
    }

    #[test]
    fn writes_raw_and_summary_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let cases = vec![required_case("ping")];
        let report = ParityReport {
            schema_version: "zyal.parity.v1".into(),
            reference: "ref".into(),
            candidate: "cand".into(),
            results: vec![ParityResult {
                case_id: "ping".into(),
                target: "cand".into(),
                status: "passed".into(),
                skipped: false,
                message: None,
                perf: None,
                ..ParityResult::default()
            }],
        };
        let artifacts = write_report_artifacts(dir.path(), "run-1", &cases, report).unwrap();
        assert!(artifacts.generated_manifest_json.exists());
        assert!(artifacts.approved_ci_txt.exists());
        assert!(artifacts.raw_jsonl.exists());
        assert!(artifacts.summary_json.exists());
        assert!(artifacts.gaps_json.exists());
        let approved = std::fs::read_to_string(artifacts.approved_ci_txt).unwrap();
        assert_eq!(approved.trim(), "ping");
        check_summary_artifact(&artifacts.summary_json, &cases).unwrap();
    }

    #[test]
    fn summary_marks_perf_missing_gate_failure() {
        let mut perf_case = required_case("perf");
        perf_case.perf = Some(ParityPerfBudget {
            p95_ms_max_ratio: Some(1.25),
        });
        let report = ParityReport {
            schema_version: "zyal.parity.v1".into(),
            reference: "ref".into(),
            candidate: "cand".into(),
            results: vec![ParityResult {
                case_id: "perf".into(),
                target: "cand".into(),
                status: "passed".into(),
                skipped: false,
                message: None,
                perf: None,
                ..ParityResult::default()
            }],
        };
        let summary = summarize_report(&[perf_case], report);
        assert_eq!(summary.status, "failed");
        assert_eq!(summary.missing_perf, 1);
    }

    #[test]
    fn parity_gap_converts_to_followup_task() {
        let gap = gaps::make_gap_for_tests(
            "failed-ping-candidate",
            "ping",
            "failed_case",
            "correctness",
            1,
            "expected PONG",
        );
        let task = parity_gap_to_followup_task(&gap, "stage-parity");
        assert_eq!(task.stage_id, "stage-parity");
        assert_eq!(task.task_kind, "parity_gap");
        assert_eq!(task.risk_level, "high");
        assert!(task.title.contains("ping"));
        assert!(task.bounded_write_scope);
    }
}
