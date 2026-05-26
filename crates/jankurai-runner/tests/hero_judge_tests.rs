use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use jankurai_runner::bootstrap_check;
use jankurai_runner::hero_judge::{HeroJudgeMissingProviderPolicy, HeroJudgeRunbook};
use jankurai_runner::hero_judge_runner::run_hero_judge_run_with_db;
use jankurai_runner::model_client::FakeModelClient;
use jankurai_runner::model_client::{kind_label, ModelCallReceipt, ModelClient};
use jankurai_runner::model_policy::ModelTaskKind;
use jekko_store::db::Db;
use serde_json::Value;
use tempfile::tempdir;

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
    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::write(
        dir.join("docs/zyal-research-loops.md"),
        "OpenQG research loops require verified evidence and receipts.",
    )
    .unwrap();
    fs::create_dir_all(dir.join("tips/rolling")).unwrap();
    fs::write(
        dir.join("tips/rolling/tip1.txt"),
        "admit falsifiable theories",
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

fn runbook() -> HeroJudgeRunbook {
    serde_yaml::from_str(
        r#"
job:
  name: openqg-hero-judge
  objective: Evolve OpenQG prompts.
hero_judge:
  generations: 1
  population:
    hero_lanes: 2
    judge_lanes: 1
    verifier_lanes: 1
    literature_lanes: 1
    red_team_lanes: 1
    max_parallel: 2
  budgets:
    model_calls: 24
    search_queries: 1
    search_pages: 2
"#,
    )
    .unwrap()
}

fn single_lane_runbook() -> HeroJudgeRunbook {
    let mut rb = runbook();
    rb.hero_judge.population.hero_lanes = 1;
    rb.hero_judge.population.judge_lanes = 1;
    rb.hero_judge.population.verifier_lanes = 1;
    rb.hero_judge.population.literature_lanes = 1;
    rb.hero_judge.population.red_team_lanes = 1;
    rb.hero_judge.population.max_parallel = 1;
    rb
}

#[derive(Clone)]
struct ScriptedModelClient {
    receipts: Arc<Mutex<VecDeque<ModelCallReceipt>>>,
}

impl ScriptedModelClient {
    fn new(receipts: Vec<ModelCallReceipt>) -> Self {
        Self {
            receipts: Arc::new(Mutex::new(receipts.into())),
        }
    }
}

#[async_trait]
impl ModelClient for ScriptedModelClient {
    async fn complete(
        &self,
        kind: ModelTaskKind,
        _prompt: &str,
        _cwd: &Path,
    ) -> anyhow::Result<ModelCallReceipt> {
        let mut receipts = self.receipts.lock().unwrap();
        let receipt = receipts.pop_front().expect("script exhausted");
        assert_eq!(receipt.kind, kind_label(kind));
        Ok(receipt)
    }
}

fn scripted_success(
    id: usize,
    kind: ModelTaskKind,
    provider: &str,
    response: &str,
) -> ModelCallReceipt {
    ModelCallReceipt {
        id: format!("receipt-{id:02}"),
        kind: kind_label(kind).to_string(),
        task_id: None,
        provider: provider.to_string(),
        model: format!("{provider}-model"),
        latency_ms: 1,
        success: true,
        cost_usd: Some(0.0),
        response: Some(response.to_string()),
        error: None,
        budget_used: None,
        budget_remaining: None,
        route: Some(kind_label(kind).to_string()),
        credential_policy: None,
        selected_credential_user_id: None,
        credential_user_id: None,
        retry_count: Some(0),
    }
}

fn scripted_failure(
    id: usize,
    kind: ModelTaskKind,
    provider: &str,
    error: &str,
) -> ModelCallReceipt {
    ModelCallReceipt {
        id: format!("receipt-{id:02}"),
        kind: kind_label(kind).to_string(),
        task_id: None,
        provider: provider.to_string(),
        model: format!("{provider}-model"),
        latency_ms: 1,
        success: false,
        cost_usd: Some(0.0),
        response: None,
        error: Some(error.to_string()),
        budget_used: None,
        budget_remaining: None,
        route: Some(kind_label(kind).to_string()),
        credential_policy: None,
        selected_credential_user_id: None,
        credential_user_id: None,
        retry_count: Some(0),
    }
}

fn read_model_outcome_states(events_path: &Path, kind: &str) -> Vec<String> {
    fs::read_to_string(events_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| event["kind"] == "model_outcome" && event["data"]["kind"] == kind)
        .map(|event| event["data"]["state"].as_str().unwrap().to_string())
        .collect()
}

fn read_model_attempt_outcome_states(events_path: &Path, kind: &str) -> Vec<String> {
    fs::read_to_string(events_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| event["kind"] == "model_attempt_outcome" && event["data"]["kind"] == kind)
        .map(|event| event["data"]["state"].as_str().unwrap().to_string())
        .collect()
}

fn first_lane_artifact(path: &Path) -> Value {
    serde_json::from_str::<Value>(&fs::read_to_string(path).unwrap())
        .unwrap()
        .as_array()
        .and_then(|items| items.first().cloned())
        .expect("expected at least one lane artifact")
}

#[tokio::test]
async fn deterministic_run_writes_required_artifacts() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let report = run_hero_judge_run_with_db(
        dir.path(),
        "hero-judge-smoke",
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        runbook(),
        Some(1),
        false,
        &FakeModelClient::success("not json but fake is allowed"),
        &db,
    )
    .await
    .unwrap();
    assert!(report.prompt_lineage_json.exists());
    assert!(report.frontier_scoreboard_json.exists());
    assert!(report.promotion_decision_json.exists());
    assert!(report.knowledge_compound_jsonl.exists());
    assert!(report.search_receipts_json.exists());
    assert!(report.quality_metrics_jsonl.exists());
    assert!(report.quality_metrics_csv.exists());
    assert!(report.quality_trend_json.exists());
    assert!(report.superreasoning_packet_json.exists());
    assert!(report.replay_receipt_json.exists());
    assert!(report.model_receipts_jsonl.exists());
    assert!(report.claim_ledger_jsonl.exists());
    assert!(report.unsupported_claims_jsonl.exists());
    assert!(report.negative_memory_jsonl.exists());
    assert!(report.headless_state_json.exists());
    assert!(report.headless_state_md.exists());
    assert!(report.complete_ok.exists());
    assert_eq!(report.knowledge_entry_count, 1);
    assert!(report.last_promotion_decision.promoted);
}

#[tokio::test]
async fn deterministic_metrics_show_quality_trend() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let mut rb = runbook();
    rb.hero_judge.generations = 2;
    let report = run_hero_judge_run_with_db(
        dir.path(),
        "hero-judge-trend",
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        rb,
        None,
        false,
        &FakeModelClient::success("not json but fake is allowed"),
        &db,
    )
    .await
    .unwrap();
    let metrics = fs::read_to_string(&report.quality_metrics_jsonl).unwrap();
    assert_eq!(metrics.lines().count(), 2);
    let trend: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report.quality_trend_json).unwrap()).unwrap();
    assert_eq!(
        trend.get("generations").and_then(serde_json::Value::as_u64),
        Some(2)
    );
    assert!(
        trend
            .get("delta_overall_quality")
            .and_then(serde_json::Value::as_f64)
            .unwrap()
            >= 0.0
    );
}

#[tokio::test]
async fn fail_missing_live_search_when_policy_requires_it() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let mut rb = runbook();
    rb.hero_judge.research.live_when_available = true;
    rb.hero_judge.research.missing_provider = HeroJudgeMissingProviderPolicy::Fail;
    let err = run_hero_judge_run_with_db(
        dir.path(),
        "hero-judge-no-search",
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        rb,
        Some(1),
        true,
        &FakeModelClient::success("{}"),
        &db,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("AGENT_SEARCH_LIVE"));
}

#[tokio::test]
async fn retryable_failure_then_success_records_retry_state() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let receipts = vec![
        scripted_failure(
            1,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            "temporary upstream unavailable",
        ),
        scripted_success(
            2,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            r#"{"summary":"literature ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.81}"#,
        ),
        scripted_success(
            3,
            ModelTaskKind::HeroGenerate,
            "live",
            r#"{"summary":"hero ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.88}"#,
        ),
        scripted_success(
            4,
            ModelTaskKind::JudgePatch,
            "live",
            r#"{"summary":"judge ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.82}"#,
        ),
        scripted_success(
            5,
            ModelTaskKind::Verifier,
            "live",
            r#"{"summary":"verifier ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.86}"#,
        ),
        scripted_success(
            6,
            ModelTaskKind::RedTeam,
            "live",
            r#"{"summary":"red team ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.2}"#,
        ),
        scripted_success(
            7,
            ModelTaskKind::MetaJudge,
            "live",
            r#"{"summary":"meta ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.87}"#,
        ),
        scripted_success(
            8,
            ModelTaskKind::KnowledgeCurate,
            "live",
            r#"{"summary":"knowledge ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.84}"#,
        ),
    ];
    let report = run_hero_judge_run_with_db(
        dir.path(),
        "hero-judge-retry",
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        single_lane_runbook(),
        Some(1),
        false,
        &ScriptedModelClient::new(receipts),
        &db,
    )
    .await
    .unwrap();
    let states = read_model_attempt_outcome_states(
        &dir.path()
            .join("target/zyal/runs/hero-judge-retry/events.jsonl"),
        "literature_synthesis",
    );
    assert_eq!(states, vec!["retryable_failure", "parsed"]);
    let parsed_states = read_model_outcome_states(
        &dir.path()
            .join("target/zyal/runs/hero-judge-retry/events.jsonl"),
        "literature_synthesis",
    );
    assert_eq!(parsed_states, vec!["parsed"]);
    assert!(report.complete_ok.exists());
}

#[tokio::test]
async fn fake_provider_uses_synthetic_provider_response() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let receipts = vec![
        scripted_success(
            1,
            ModelTaskKind::LiteratureSynthesis,
            "fake",
            "not json but fake is allowed",
        ),
        scripted_success(
            2,
            ModelTaskKind::HeroGenerate,
            "live",
            r#"{"summary":"hero ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.88}"#,
        ),
        scripted_success(
            3,
            ModelTaskKind::JudgePatch,
            "live",
            r#"{"summary":"judge ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.82}"#,
        ),
        scripted_success(
            4,
            ModelTaskKind::Verifier,
            "live",
            r#"{"summary":"verifier ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.86}"#,
        ),
        scripted_success(
            5,
            ModelTaskKind::RedTeam,
            "live",
            r#"{"summary":"red team ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.2}"#,
        ),
        scripted_success(
            6,
            ModelTaskKind::MetaJudge,
            "live",
            r#"{"summary":"meta ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.87}"#,
        ),
        scripted_success(
            7,
            ModelTaskKind::KnowledgeCurate,
            "live",
            r#"{"summary":"knowledge ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.84}"#,
        ),
    ];
    let report = run_hero_judge_run_with_db(
        dir.path(),
        "hero-judge-fake",
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        single_lane_runbook(),
        Some(1),
        false,
        &ScriptedModelClient::new(receipts),
        &db,
    )
    .await
    .unwrap();
    let literature = first_lane_artifact(&report.output_dir.join("generation-001/literature.json"));
    assert_eq!(
        literature.get("summary").and_then(Value::as_str),
        Some("deterministic literature_synthesis summary")
    );
    let states = read_model_attempt_outcome_states(
        &dir.path()
            .join("target/zyal/runs/hero-judge-fake/events.jsonl"),
        "literature_synthesis",
    );
    assert_eq!(states, vec!["fake_provider_synthetic_response"]);
}

#[tokio::test]
async fn live_parse_substitution_records_storage_safe_substitution() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let receipts = vec![
        scripted_success(
            1,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            "this is not json",
        ),
        scripted_success(
            2,
            ModelTaskKind::HeroGenerate,
            "live",
            r#"{"summary":"hero ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.88}"#,
        ),
        scripted_success(
            3,
            ModelTaskKind::JudgePatch,
            "live",
            r#"{"summary":"judge ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.82}"#,
        ),
        scripted_success(
            4,
            ModelTaskKind::Verifier,
            "live",
            r#"{"summary":"verifier ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.86}"#,
        ),
        scripted_success(
            5,
            ModelTaskKind::RedTeam,
            "live",
            r#"{"summary":"red team ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.2}"#,
        ),
        scripted_success(
            6,
            ModelTaskKind::MetaJudge,
            "live",
            r#"{"summary":"meta ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.87}"#,
        ),
        scripted_success(
            7,
            ModelTaskKind::KnowledgeCurate,
            "live",
            r#"{"summary":"knowledge ok","claims":["c"],"questions":["q"],"rubric":["r"],"evidence_refs":["e"],"score":0.84}"#,
        ),
    ];
    let report = run_hero_judge_run_with_db(
        dir.path(),
        "hero-judge-parse",
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        single_lane_runbook(),
        Some(1),
        false,
        &ScriptedModelClient::new(receipts),
        &db,
    )
    .await
    .unwrap();
    let literature = first_lane_artifact(&report.output_dir.join("generation-001/literature.json"));
    assert_eq!(
        literature.get("summary").and_then(Value::as_str),
        Some("live literature_synthesis response completed but required storage-safe JSON substitute")
    );
    let states = read_model_attempt_outcome_states(
        &dir.path()
            .join("target/zyal/runs/hero-judge-parse/events.jsonl"),
        "literature_synthesis",
    );
    assert_eq!(states, vec!["live_parse_substitution"]);
}

#[tokio::test]
async fn live_mode_invalid_json_blocks_instead_of_substituting() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let receipts = vec![
        scripted_success(
            1,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            "this is not json",
        ),
        scripted_success(
            2,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            "still not json",
        ),
        scripted_success(
            3,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            "not json on final attempt",
        ),
    ];
    let err = run_hero_judge_run_with_db(
        dir.path(),
        "hero-judge-live-parse-block",
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        single_lane_runbook(),
        Some(1),
        true,
        &ScriptedModelClient::new(receipts),
        &db,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("live model response was not parseable JSON"));
    let states = read_model_attempt_outcome_states(
        &dir.path()
            .join("target/zyal/runs/hero-judge-live-parse-block/events.jsonl"),
        "literature_synthesis",
    );
    assert_eq!(
        states,
        vec!["retryable_failure", "retryable_failure", "final_block"]
    );
}

#[tokio::test]
async fn retryable_failure_exhaustion_blocks_run() {
    let dir = tempdir().unwrap();
    bootstrap_repo(dir.path());
    let db = Db::open_in_memory().unwrap();
    let receipts = vec![
        scripted_failure(
            1,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            "temporary upstream unavailable",
        ),
        scripted_failure(
            2,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            "temporary upstream unavailable",
        ),
        scripted_failure(
            3,
            ModelTaskKind::LiteratureSynthesis,
            "live",
            "temporary upstream unavailable",
        ),
    ];
    let err = run_hero_judge_run_with_db(
        dir.path(),
        "hero-judge-blocked",
        &dir.path().join("agent/zyal/openqg-hero-judge-evolve.zyal"),
        single_lane_runbook(),
        Some(1),
        false,
        &ScriptedModelClient::new(receipts),
        &db,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("model call failed"));
    let states = read_model_attempt_outcome_states(
        &dir.path()
            .join("target/zyal/runs/hero-judge-blocked/events.jsonl"),
        "literature_synthesis",
    );
    assert_eq!(
        states,
        vec!["retryable_failure", "retryable_failure", "final_block"]
    );
}
