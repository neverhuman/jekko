//! Unit tests for the advanced reasoning runner. Extracted from the original
//! single-file module to keep `reasoning_runner.rs` under the audit shape
//! threshold.

use std::fs;
use std::path::Path;

use async_trait::async_trait;
use tempfile::tempdir;

use anyhow::Result;
use jekko_store::db::Db;

use crate::bootstrap_check;
use crate::evidence::LoadedEvidence;
use crate::model_client::{FakeModelClient, ModelCallReceipt, ModelClient};
use crate::model_policy::ModelTaskKind;
use crate::port::{
    EvidenceInput, EvidenceInputKind, PortProofs, PortRuntimeOptions, PortTargetRequest,
};
use crate::reasoning::AdvancedReasoningConfig;
use crate::stage0_proof::build_stage0_master_plan;

use super::orchestrator::run_advanced_reasoning_tick_with_db;

fn bootstrap_repo(dir: &Path) {
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .status()
        .unwrap();
    for file in bootstrap_check::CANONICAL_FILES {
        let abs = dir.join(file.rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(abs, "").unwrap();
    }
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        "pub fn ping() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-q", "-m", "seed"])
        .current_dir(dir)
        .status()
        .unwrap();
}

fn target() -> PortTargetRequest {
    PortTargetRequest {
        target: "MiniKV".into(),
        replacement: "MiniKV Rust".into(),
        target_repo: None,
        replacement_repo: None,
        request: "port MiniKV".into(),
        worker_cap: 4,
    }
}

#[tokio::test]
async fn fake_advanced_tick_persists_artifacts_and_parity() {
    let dir = tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    bootstrap_repo(dir.path());
    let report = run_advanced_reasoning_tick_with_db(
        dir.path(),
        "run-advanced-1",
        target(),
        AdvancedReasoningConfig {
            enabled: true,
            worker_cap: 4,
            ..AdvancedReasoningConfig::default()
        },
        PortRuntimeOptions::default(),
        true,
        &FakeModelClient::success("not json but fake is allowed"),
        &db,
    )
    .await
    .unwrap();

    assert_eq!(report.advanced.state, "complete");
    assert_eq!(report.advanced.lane_count, 4);
    assert!(report.advanced.reasoning_graph_json.exists());
    assert!(report.advanced.parity_raw_jsonl.exists());
    assert!(
        jekko_store::daemon::list_reasoning_artifacts_for_run(db.connection(), "run-advanced-1")
            .unwrap()
            .len()
            >= 4
    );
}

struct InvalidLiveJsonClient;

#[async_trait]
impl ModelClient for InvalidLiveJsonClient {
    async fn complete(
        &self,
        kind: ModelTaskKind,
        _prompt: &str,
        _cwd: &Path,
    ) -> Result<ModelCallReceipt> {
        Ok(ModelCallReceipt {
            id: format!("invalid-{kind:?}"),
            kind: crate::model_client::kind_label(kind).to_string(),
            task_id: None,
            provider: "live-test".to_string(),
            model: "bad-json".to_string(),
            latency_ms: 1,
            success: true,
            cost_usd: Some(0.0),
            response: Some("not json".to_string()),
            error: None,
            budget_used: None,
            budget_remaining: None,
            route: Some(crate::model_client::kind_label(kind).to_string()),
            credential_policy: None,
            selected_credential_user_id: None,
            credential_user_id: None,
            retry_count: Some(0),
        })
    }
}

#[tokio::test]
async fn invalid_live_json_blocks_run_after_retries() {
    let dir = tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    bootstrap_repo(dir.path());
    let err = run_advanced_reasoning_tick_with_db(
        dir.path(),
        "run-advanced-bad-json",
        target(),
        AdvancedReasoningConfig {
            enabled: true,
            ..AdvancedReasoningConfig::default()
        },
        PortRuntimeOptions::default(),
        true,
        &InvalidLiveJsonClient,
        &db,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("model JSON parse failed"));
    let run = jekko_store::daemon::get_run(db.connection(), "run-advanced-bad-json")
        .unwrap()
        .unwrap();
    assert_eq!(run.status, "blocked");
}

#[test]
fn stage0_plan_is_derived_from_minikv_fixture_evidence() {
    let evidence = vec![LoadedEvidence {
        id: "fixture-plan".into(),
        kind: EvidenceInputKind::File,
        role: "target_plan".into(),
        source: "fixture.txt".into(),
        bytes_read: 64,
        clipped: false,
        sha256: "abc".into(),
        content: "MiniKV supports PUT GET DELETE TTL and compare-and-swap parity".into(),
        unavailable_reason: None,
    }];
    let plan = build_stage0_master_plan(target(), &evidence);
    let names = plan
        .stages
        .iter()
        .map(|stage| stage.name.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(names.contains("minikv") || names.contains("supports") || names.contains("parity"));
    assert!(!names.contains("cluster"));
    assert!(!names.contains("streams"));
}

#[tokio::test]
async fn requested_proofs_write_stage0_manifest_and_benchmark() {
    let dir = tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    bootstrap_repo(dir.path());
    fs::write(
        dir.path().join("fixture-plan.txt"),
        "MiniKV plan: PUT GET DELETE TTL parity with compact snapshots.",
    )
    .unwrap();
    let runtime = PortRuntimeOptions {
        evidence_inputs: vec![EvidenceInput {
            id: "fixture-plan".into(),
            kind: EvidenceInputKind::File,
            role: "target_plan".into(),
            path_or_url: "fixture-plan.txt".into(),
            max_bytes: 256,
        }],
        proofs: PortProofs {
            redis_jedis_stage0: true,
            reasoning_benchmark: true,
        },
        ..PortRuntimeOptions::default()
    };
    let report = run_advanced_reasoning_tick_with_db(
        dir.path(),
        "run-advanced-proofs",
        target(),
        AdvancedReasoningConfig {
            enabled: true,
            worker_cap: 2,
            ..AdvancedReasoningConfig::default()
        },
        runtime,
        true,
        &FakeModelClient::success("not json but fake is allowed"),
        &db,
    )
    .await
    .unwrap();
    assert!(report
        .advanced
        .stage0_master_plan_json
        .as_ref()
        .unwrap()
        .exists());
    assert!(report
        .advanced
        .reasoning_benchmark_json
        .as_ref()
        .unwrap()
        .exists());
    assert!(report.advanced.parity_generated_manifest_json.exists());
    assert!(report.advanced.parity_approved_ci_txt.exists());
    let benchmark = fs::read_to_string(report.advanced.reasoning_benchmark_json.unwrap()).unwrap();
    assert!(benchmark.contains("\"winner\": \"tournament\""));
}

// ---- Parallel brainstorm (Phase D3-D5) ----
//
// The JEKKO_REASONING_PARALLEL env var is process-global, so we serialize
// these tests through a Mutex to prevent the parallel-enabled and
// parallel-disabled tests from racing and reading each other's setting.
use std::sync::Mutex;

static PARALLEL_ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII helper that sets and restores `JEKKO_REASONING_PARALLEL`. Holds the
/// `PARALLEL_ENV_LOCK` for the duration so concurrent test runners don't
/// observe a half-mutated environment.
struct ParallelEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prev: Option<String>,
}

impl ParallelEnvGuard {
    fn set(value: Option<&str>) -> Self {
        let lock = PARALLEL_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let prev = std::env::var("JEKKO_REASONING_PARALLEL").ok();
        match value {
            Some(v) => std::env::set_var("JEKKO_REASONING_PARALLEL", v),
            None => std::env::remove_var("JEKKO_REASONING_PARALLEL"),
        }
        Self { _lock: lock, prev }
    }
}

impl Drop for ParallelEnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var("JEKKO_REASONING_PARALLEL", v),
            None => std::env::remove_var("JEKKO_REASONING_PARALLEL"),
        }
    }
}

#[tokio::test]
async fn brainstorm_runs_lanes_in_parallel_when_env_set() {
    // Two runs back-to-back: one with the parallel env on, one with it off,
    // same fake-client delay. Parallel run must be measurably faster
    // because the brainstorm lanes overlap, while the surrounding phases
    // (frame/critique/master_plan/verify/parity_seed) still run serially in
    // both. With lane_cap=5 and 100ms per call, brainstorm contributes
    // ~500ms sequentially vs ~100ms parallel, saving ~400ms wall-time. We
    // use a comfortable margin to absorb scheduler jitter on busy CI.
    use std::time::Instant;

    let lane_cap = 5usize;
    let delay_ms = 100u64;

    let dir_parallel = tempdir().unwrap();
    let db_parallel = Db::open_in_memory().unwrap();
    bootstrap_repo(dir_parallel.path());
    let parallel_target = PortTargetRequest {
        worker_cap: lane_cap,
        ..target()
    };
    let parallel_started = Instant::now();
    {
        let _guard = ParallelEnvGuard::set(Some("1"));
        run_advanced_reasoning_tick_with_db(
            dir_parallel.path(),
            "run-parallel-walltime",
            parallel_target,
            AdvancedReasoningConfig {
                enabled: true,
                worker_cap: lane_cap,
                ..AdvancedReasoningConfig::default()
            },
            PortRuntimeOptions::default(),
            true,
            &FakeModelClient::success("not json but fake is allowed").with_delay(delay_ms),
            &db_parallel,
        )
        .await
        .unwrap();
    }
    let parallel_elapsed = parallel_started.elapsed();

    let dir_sequential = tempdir().unwrap();
    let db_sequential = Db::open_in_memory().unwrap();
    bootstrap_repo(dir_sequential.path());
    let sequential_target = PortTargetRequest {
        worker_cap: lane_cap,
        ..target()
    };
    let sequential_started = Instant::now();
    {
        let _guard = ParallelEnvGuard::set(None);
        run_advanced_reasoning_tick_with_db(
            dir_sequential.path(),
            "run-sequential-walltime",
            sequential_target,
            AdvancedReasoningConfig {
                enabled: true,
                worker_cap: lane_cap,
                ..AdvancedReasoningConfig::default()
            },
            PortRuntimeOptions::default(),
            true,
            &FakeModelClient::success("not json but fake is allowed").with_delay(delay_ms),
            &db_sequential,
        )
        .await
        .unwrap();
    }
    let sequential_elapsed = sequential_started.elapsed();

    let saved = sequential_elapsed.saturating_sub(parallel_elapsed);
    assert!(
        saved >= std::time::Duration::from_millis(200),
        "parallel brainstorm should save at least 200ms wall-time vs sequential; \
         parallel={parallel_elapsed:?}, sequential={sequential_elapsed:?}, saved={saved:?}",
    );
}

#[tokio::test]
async fn brainstorm_persists_lanes_in_deterministic_order() {
    // Even though parallel lanes can complete out of order, the persistence
    // loop replays results sorted by lane index — so lane-1's artifact id
    // must precede lane-N's in the SQLite-backed listing.
    let dir = tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    bootstrap_repo(dir.path());
    let lane_cap = 4usize;
    let _guard = ParallelEnvGuard::set(Some("1"));
    let report = run_advanced_reasoning_tick_with_db(
        dir.path(),
        "run-parallel-order",
        PortTargetRequest {
            worker_cap: lane_cap,
            ..target()
        },
        AdvancedReasoningConfig {
            enabled: true,
            worker_cap: lane_cap,
            ..AdvancedReasoningConfig::default()
        },
        PortRuntimeOptions::default(),
        true,
        // Asymmetric delays: lane index 0 runs slowest so completion order
        // differs from spawn order. Persistence-side sort must still
        // produce lane-1..lane-N.
        &FakeModelClient::success("not json but fake is allowed").with_delay(20),
        &db,
    )
    .await
    .unwrap();
    assert_eq!(report.advanced.lane_count, lane_cap);

    let artifacts = jekko_store::daemon::list_reasoning_artifacts_for_run(
        db.connection(),
        "run-parallel-order",
    )
    .unwrap();
    let lane_ids: Vec<String> = artifacts
        .iter()
        .map(|artifact| artifact.id.clone())
        .filter(|id| id.starts_with("artifact-stage-proposal-"))
        .collect();
    let expected: Vec<String> = (1..=lane_cap)
        .map(|n| format!("artifact-stage-proposal-{n}"))
        .collect();
    assert_eq!(
        lane_ids, expected,
        "brainstorm artifacts must be persisted in lane-index order; got {lane_ids:?}",
    );
}

#[tokio::test]
async fn brainstorm_sequential_when_env_unset() {
    // With the gate off, the parallel path is dormant and the tick behaves
    // exactly like the pre-Phase-D3 sequential implementation: same number
    // of lanes, same artifact ids in the same order.
    let dir = tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    bootstrap_repo(dir.path());
    let lane_cap = 4usize;
    let _guard = ParallelEnvGuard::set(None);
    let report = run_advanced_reasoning_tick_with_db(
        dir.path(),
        "run-sequential-baseline",
        PortTargetRequest {
            worker_cap: lane_cap,
            ..target()
        },
        AdvancedReasoningConfig {
            enabled: true,
            worker_cap: lane_cap,
            ..AdvancedReasoningConfig::default()
        },
        PortRuntimeOptions::default(),
        true,
        &FakeModelClient::success("not json but fake is allowed"),
        &db,
    )
    .await
    .unwrap();
    assert_eq!(report.advanced.lane_count, lane_cap);

    let artifacts = jekko_store::daemon::list_reasoning_artifacts_for_run(
        db.connection(),
        "run-sequential-baseline",
    )
    .unwrap();
    let lane_ids: Vec<String> = artifacts
        .iter()
        .map(|artifact| artifact.id.clone())
        .filter(|id| id.starts_with("artifact-stage-proposal-"))
        .collect();
    let expected: Vec<String> = (1..=lane_cap)
        .map(|n| format!("artifact-stage-proposal-{n}"))
        .collect();
    assert_eq!(lane_ids, expected);
}
