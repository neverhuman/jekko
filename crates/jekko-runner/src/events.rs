//! NDJSON event sink. Append-only line stream at
//! `target/zyal/runs/<run_id>/events.jsonl`, mirrored to
//! `target/zyal/runner-events.jsonl`. Each line ≤ 512 bytes so daemon-side
//! tailers can budget their read window. The schema is deliberately flat:
//! every event carries `ts` + `kind` + `run_id`, plus a free-form `data`
//! object for kind-specific fields.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EVENT_FILE_REL: &str = "target/zyal/runner-events.jsonl";
pub const RUNS_DIR_REL: &str = "target/zyal/runs";
pub const MAX_LINE_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunnerEventKind {
    RunStarted,
    BrainstormStarted,
    ReasoningState,
    ReasoningArtifact,
    ReasoningLane,
    MemoryCapsule,
    PhaseFinalized,
    TaskAssigned,
    WorkerStarted,
    WorkerPass,
    WorkerFail,
    ProofPassed,
    ProofFailed,
    AuditResult,
    ParityResult,
    ParityGap,
    ParityManifestGenerated,
    ModelAttempt,
    ModelAttemptOutcome,
    ModelOutcome,
    LiveBudget,
    BenchmarkResult,
    HeroJudgeGeneration,
    ResearchReceipt,
    HeroCandidate,
    JudgePatch,
    VerifierScore,
    PromotionDecision,
    KnowledgeCompounded,
    CommitLanded,
    RebaseConflict,
    WorkerRollback,
    TaskQuarantined,
    GcPruned,
    RunFinished,
    BootstrapRequired,
    Heartbeat,
    /// Watcher detected no progress event from a worker within the stall
    /// threshold (default 5 minutes).
    WorkerStall,
    /// Watcher revoked a worker's task lease after stall; orchestrator should
    /// reassign the task to a fresh worker.
    WorkerQuarantine,
    /// `jekko watch` (or equivalent) started observing this run's events.
    WatcherStarted,
    /// Watcher applied a remediation rule (e.g. wrote a Negative-memory
    /// capsule, escalated to Critique, requested a provider switch).
    RemediationTriggered,
    /// Jankurai audit hard-findings count increased mid-run — phase signoff
    /// should block until cleared.
    JankuraiRegression,
    /// Three or more consecutive model attempts at the same task kind
    /// returned an empty response (response_bytes == 0). Distinct from
    /// `model_failure` / `retryable_failure` because the subprocess
    /// completed successfully — the model spoke, but said nothing.
    /// Typically a content-side issue (prompt too long, output budget
    /// exceeded, content filter) — the recommended remediation is to
    /// declare a stronger `quality_band` on the affected stage.
    EmptyResponseStreak,
    /// Live per-node status change (one flattened agent-flow box). Drives the
    /// SPA's per-box status at scale. `data`: `{node_id, status, progress?, tokens?}`.
    AgentStatus,
    /// A global KPI box's value advanced. `data`: `{kpi: {id, name, value, unit, status}}`.
    KpiUpdate,
    /// A watcher box's value/equation advanced. `data`: `{watcher: {id, kind, label, expr, current_value}}`.
    WatcherUpdate,
    /// A data-feed source box received a new tick. `data`: `{feed: {id, kind, label, latest_value}}`.
    FeedTick,
    /// A watcher/plot series point (compact; full series lives in storage).
    /// `data`: `{watcher_id, x, y}`.
    PlotUpdate,
    /// An artifact finished rendering (e.g. pdf→png). `data`: `{artifact_id, content_url, mime_type}`.
    ArtifactRendered,

    // ===== ZYAL Power roadmap vocabulary (M0) =====
    // Pure vocabulary: emitters land in the milestone noted. Every variant must
    // keep its serialized line ≤512 bytes (IDs + scalars only; full payloads go
    // to receipts/storage). Folds live in `watcher/metrics.rs`; frame builders
    // in `watcher/live.rs`.

    // ---- routing topology (M8) ----
    /// A route resolved its candidate set + policy. `data`: `{route_id, policy, n}`.
    RoutingDecision,
    /// One provider attempt within a route. `data`: `{route_id, candidate_id, phase}`.
    ProviderCall,
    /// A speculative/backup provider call was canceled after a winner committed.
    /// `data`: `{route_id, candidate_id, estimated_cost_saved_usd}`.
    ProviderCallCanceled,
    /// A cascade/quorum route escalated to a stronger candidate.
    RouteEscalated,
    /// A route judge ranked candidates. `data`: `{route_id, winner}`.
    RouteJudged,
    /// A route committed a winner. `data`: `{route_id, candidate_id}`.
    RouteWinner,
    /// Shadow-sampled regret for a route policy. `data`: `{route_id, regret}`.
    RouteRegretMeasured,
    /// A compiled budget gate decision. `data`: `{node_id, allow, scope}`.
    BudgetGateEvaluated,
    /// A compiled health/circuit-breaker gate decision. `data`: `{node_id, healthy}`.
    HealthGateEvaluated,

    // ---- tool kernel (M9) ----
    /// A tool call advanced a phase (queued|authorizing|denied|started|progress|
    /// retrying|succeeded|failed|cached|misused). `data`: `{tool_id, node_id, phase}`.
    ToolCallUpdate,

    // ---- tournament kernel (M10) ----
    /// A tournament generation advanced. `data`: `{tournament_id, gen, best}`.
    TournamentGeneration,
    /// Blind cross-provider judging could not be guaranteed; promotion capped.
    JudgeBlindnessDegraded,
    /// A promotion gate decision. `data`: `{tournament_id, promoted}`.
    PromotionGate,
    /// A nested ZYAL workflow call started. `data`: `{node_id, ref, mode}`.
    ChildWorkflowStarted,
    /// A nested ZYAL workflow call completed. `data`: `{node_id, ok}`.
    ChildWorkflowCompleted,
    /// A nested ZYAL workflow call failed validation/execution. `data`: `{node_id, reason}`.
    ChildWorkflowFailed,
    /// A child workflow published an artifact. `data`: `{node_id, kind, path}`.
    ArtifactPublished,
    /// A paper-builder review epoch completed. `data`: `{node_id, epoch, reviewers}`.
    ReviewEpochCompleted,

    // ---- pattern compiler (M7) ----
    PatternStarted,
    PatternUpdate,
    ScatterGatherUpdate,
    LoopUntilDryUpdate,
    /// A loop_until_dry loop emitted its dry certificate (marginal-utility exhausted).
    DryCertificate,

    // ---- cost governor (M16) ----
    /// Budget burn advanced. `data`: `{scope, spent_usd, ceiling_usd?}`.
    CostUpdate,
    /// A hard budget ceiling was hit (unbypassable). `data`: `{scope, spent_usd}`.
    BudgetExceeded,

    // ---- control-command API (M16) ----
    ControlCommandSubmitted,
    ControlCommandValidated,
    ControlCommandRejected,
    ControlCommandRequiresGate,
    ControlCommandApplied,

    // ---- trust pipeline / taint (M6) ----
    /// Tainted data was blocked from arming an action. `data`: `{node_id, action, taint}`.
    TaintViolation,
    /// A capability lease was granted. `data`: `{lease_id, tool_id, node_id}`.
    LeaseGranted,
    /// A capability lease was denied. `data`: `{tool_id, node_id, reason}`.
    LeaseDenied,
    /// The host-enforced arming gate ran. `data`: `{node_id, action, armed}`.
    DecideArm,

    // ---- durable run store (M6) ----
    /// A run snapshot was written. `data`: `{at_offset}`.
    Checkpoint,
    ResumeFromCheckpoint,
    ReplayStarted,
    /// A fork was created from an offset. `data`: `{from_run_id, from_offset, new_run_id}`.
    ForkCreated,

    // ---- signals / sources / research (M11–M13) ----
    /// A watcher breach fired a typed control trigger over a pre-declared edge.
    /// `data`: `{watcher_id, action, target}`.
    ControlTrigger,
    /// An anti-Goodhart guard advanced. `data`: `{kpi_id, guard, alert}`.
    GoodhartUpdate,
    /// A research claim changed status. `data`: `{claim_id, status}`.
    ResearchClaim,

    // ---- artifacts (M14) ----
    /// A content-addressed artifact version was recorded. `data`: `{artifact_id, version_id}`.
    ArtifactVersioned,
}

pub use RunnerEventKind as EventKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub ts: u64,
    pub kind: EventKind,
    pub run_id: String,
    /// Monotonic per-run sequence number. The spine of F6 replay/fork and the
    /// no-relayout `(id, rev)` materializer. Serialized as the compact key `o`
    /// to spend few of the 512 byte budget; defaults to 0 for legacy lines.
    #[serde(default, rename = "o")]
    pub event_offset: u64,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

pub struct EventSink {
    path: PathBuf,
    mirror_path: PathBuf,
    run_id: String,
    offset: AtomicU64,
}

impl EventSink {
    pub fn open(repo_root: &Path, run_id: &str) -> Result<Self> {
        let path = repo_root.join(run_event_file_rel(run_id));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir -p {}", parent.display()))?;
        }
        let mirror_path = repo_root.join(EVENT_FILE_REL);
        if let Some(parent) = mirror_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir -p {}", parent.display()))?;
        }
        Ok(Self {
            path,
            mirror_path,
            run_id: run_id.to_string(),
            offset: AtomicU64::new(0),
        })
    }

    /// The next offset this sink will assign (i.e. the count emitted so far).
    pub fn next_offset(&self) -> u64 {
        self.offset.load(Ordering::SeqCst)
    }

    pub fn emit(&self, kind: EventKind, data: Value) -> Result<()> {
        let event = Event {
            ts: now_epoch_secs(),
            kind,
            run_id: self.run_id.clone(),
            event_offset: self.offset.fetch_add(1, Ordering::SeqCst),
            data,
        };
        let line = serde_json::to_string(&event).context("serialize event")?;
        if line.len() > MAX_LINE_BYTES {
            return Err(anyhow::anyhow!(
                "runner event exceeds {} bytes ({} bytes): {}",
                MAX_LINE_BYTES,
                line.len(),
                truncate_for_error(&line),
            ));
        }
        append_line(&self.path, &line)?;
        append_line(&self.mirror_path, &line)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mirror_path(&self) -> &Path {
        &self.mirror_path
    }
}

pub fn run_event_file_rel(run_id: &str) -> PathBuf {
    PathBuf::from(RUNS_DIR_REL)
        .join(run_id)
        .join("events.jsonl")
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn now_epoch_secs() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    }
}

fn truncate_for_error(line: &str) -> &str {
    let cap = line
        .char_indices()
        .nth(120)
        .map(|(i, _)| i)
        .unwrap_or(line.len());
    &line[..cap]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn emits_one_line_per_event() {
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-1").unwrap();
        sink.emit(EventKind::RunStarted, json!({"pool_size": 4}))
            .unwrap();
        sink.emit(EventKind::WorkerStarted, json!({"worker": "w-01"}))
            .unwrap();
        let text = fs::read_to_string(sink.path()).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("run_started"));
        assert!(text.contains("worker_started"));
        let mirror = fs::read_to_string(sink.mirror_path()).unwrap();
        assert_eq!(mirror.lines().count(), 2);
    }

    #[test]
    fn rejects_lines_over_512_bytes() {
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-1").unwrap();
        let huge = "x".repeat(600);
        let err = sink
            .emit(EventKind::WorkerPass, json!({"blob": huge}))
            .unwrap_err();
        assert!(err.to_string().contains("exceeds"));
        // and nothing was written
        assert!(!sink.path().exists() || fs::read_to_string(sink.path()).unwrap().is_empty());
    }

    #[test]
    fn lines_are_parseable_back_into_event() {
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-1").unwrap();
        sink.emit(EventKind::CommitLanded, json!({"sha": "abc123"}))
            .unwrap();
        let text = fs::read_to_string(sink.path()).unwrap();
        let event: Event = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(event.kind, EventKind::CommitLanded);
        assert_eq!(event.run_id, "run-1");
        assert_eq!(event.data["sha"], "abc123");
    }

    #[test]
    fn uses_per_run_event_path() {
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-42").unwrap();
        assert!(sink
            .path()
            .ends_with("target/zyal/runs/run-42/events.jsonl"));
    }

    #[test]
    fn assigns_monotonic_event_offsets() {
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-off").unwrap();
        assert_eq!(sink.next_offset(), 0);
        sink.emit(EventKind::RunStarted, json!({})).unwrap();
        sink.emit(EventKind::WorkerStarted, json!({"w": "w-1"}))
            .unwrap();
        sink.emit(EventKind::RunFinished, json!({})).unwrap();
        assert_eq!(sink.next_offset(), 3);
        let offsets: Vec<u64> = fs::read_to_string(sink.path())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Event>(l).unwrap().event_offset)
            .collect();
        assert_eq!(offsets, vec![0, 1, 2]);
    }

    #[test]
    fn legacy_lines_without_offset_default_to_zero() {
        let line = r#"{"ts":1,"kind":"run_started","run_id":"r","data":{}}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert_eq!(ev.event_offset, 0);
    }

    #[test]
    fn new_roadmap_event_kinds_emit_under_the_line_cap() {
        // Pure-vocabulary smoke test (M0): every new kind, emitted with a
        // representative IDs-only payload, must serialize to ≤512 bytes.
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-vocab").unwrap();
        let cases: &[(EventKind, Value)] = &[
            (
                EventKind::RoutingDecision,
                json!({"route_id": "route_fusion", "policy": "fusion_sample", "n": 4}),
            ),
            (
                EventKind::ProviderCall,
                json!({"route_id": "route_fusion", "candidate_id": "draft-2", "phase": "started"}),
            ),
            (
                EventKind::ProviderCallCanceled,
                json!({"route_id": "route_fusion", "candidate_id": "draft-3", "estimated_cost_saved_usd": 0.0123}),
            ),
            (
                EventKind::RouteWinner,
                json!({"route_id": "route_fusion", "candidate_id": "fusion-0"}),
            ),
            (
                EventKind::RouteRegretMeasured,
                json!({"route_id": "route_fusion", "regret": 0.018}),
            ),
            (
                EventKind::BudgetGateEvaluated,
                json!({"node_id": "budget_gate/0", "allow": true, "scope": "run"}),
            ),
            (
                EventKind::ToolCallUpdate,
                json!({"tool_id": "code.exec", "node_id": "tool/solve", "phase": "succeeded"}),
            ),
            (
                EventKind::TournamentGeneration,
                json!({"tournament_id": "hero", "gen": 3, "best": 0.91}),
            ),
            (
                EventKind::JudgeBlindnessDegraded,
                json!({"tournament_id": "hero", "reason": "single_provider"}),
            ),
            (
                EventKind::DryCertificate,
                json!({"pattern_id": "research_loop", "rounds": 4}),
            ),
            (
                EventKind::CostUpdate,
                json!({"scope": "run", "spent_usd": 1.42, "ceiling_usd": 5.0}),
            ),
            (
                EventKind::BudgetExceeded,
                json!({"scope": "run", "spent_usd": 5.01}),
            ),
            (
                EventKind::ControlCommandApplied,
                json!({"command_id": "c-1", "kind": "pause"}),
            ),
            (
                EventKind::TaintViolation,
                json!({"node_id": "writer/0", "action": "exec_shell", "taint": "web_content"}),
            ),
            (
                EventKind::LeaseGranted,
                json!({"lease_id": "l-1", "tool_id": "code.exec", "node_id": "tool/solve"}),
            ),
            (
                EventKind::DecideArm,
                json!({"node_id": "publish/0", "action": "arm_host_action", "armed": false}),
            ),
            (EventKind::Checkpoint, json!({"at_offset": 128})),
            (
                EventKind::ForkCreated,
                json!({"from_run_id": "r1", "from_offset": 64, "new_run_id": "r1-fork"}),
            ),
            (
                EventKind::ControlTrigger,
                json!({"watcher_id": "convergence", "action": "widen", "target": "hero"}),
            ),
            (
                EventKind::GoodhartUpdate,
                json!({"kpi_id": "quality", "guard": "holdout", "alert": false}),
            ),
            (
                EventKind::ResearchClaim,
                json!({"claim_id": "c-42", "status": "verified"}),
            ),
            (
                EventKind::ArtifactVersioned,
                json!({"artifact_id": "report_pdf", "version_id": "v_abc123def456"}),
            ),
        ];
        for (kind, data) in cases {
            sink.emit(*kind, data.clone())
                .unwrap_or_else(|e| panic!("{kind:?} should emit under the cap: {e}"));
        }
        assert_eq!(sink.next_offset(), cases.len() as u64);
    }
}
