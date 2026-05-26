//! NDJSON event sink. Append-only line stream at
//! `target/zyal/runs/<run_id>/events.jsonl`, mirrored to
//! `target/zyal/runner-events.jsonl`. Each line ≤ 512 bytes so daemon-side
//! tailers can budget their read window. The schema is deliberately flat:
//! every event carries `ts` + `kind` + `run_id`, plus a free-form `data`
//! object for kind-specific fields.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EVENT_FILE_REL: &str = "target/zyal/runner-events.jsonl";
pub const RUNS_DIR_REL: &str = "target/zyal/runs";
pub const MAX_LINE_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub ts: u64,
    pub kind: EventKind,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

pub struct EventSink {
    path: PathBuf,
    mirror_path: PathBuf,
    run_id: String,
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
        })
    }

    pub fn emit(&self, kind: EventKind, data: Value) -> Result<()> {
        let event = Event {
            ts: now_epoch_secs(),
            kind,
            run_id: self.run_id.clone(),
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

fn now_epoch_secs() -> u64 {
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
}
