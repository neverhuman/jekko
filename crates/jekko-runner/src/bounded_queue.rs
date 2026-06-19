//! Bounded, durable job queue with a FIXED worker pool.
//!
//! Many producers enqueue tasks into the durable `daemon_task` SQLite queue; a
//! fixed pool of *exactly* `max_workers` OS threads drains it. The pool is the
//! hard cap by construction — no worker is ever spawned beyond the configured
//! size. The lease protocol in [`jekko_store::daemon`] (claim → run → complete,
//! with expired-lease reclaim) guarantees every task is executed by exactly one
//! worker at a time and that a crashed worker's in-flight task is recovered.
//!
//! Workers execute a claimed task through a [`JobRunner`]; production wires this
//! to a live `jekko run` (real model + tools), while tests inject a closure.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jekko_store::daemon::{
    claim_next_task, complete_task, count_tasks_by_status, release_task, DaemonTaskRow,
};
use jekko_store::Db;

/// Terminal disposition a [`JobRunner`] reports for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Finished successfully → task marked `done`.
    Done,
    /// Failed → released for another attempt until `max_attempts`, then `failed`.
    Failed,
}

/// Outcome of executing a single task.
#[derive(Debug, Clone)]
pub struct JobOutcome {
    /// Terminal disposition.
    pub status: JobStatus,
    /// Result payload persisted on the task (`promotion_result_json`).
    pub result: serde_json::Value,
}

impl JobOutcome {
    /// A successful outcome carrying `result`.
    pub fn done(result: serde_json::Value) -> Self {
        Self {
            status: JobStatus::Done,
            result,
        }
    }

    /// A failed outcome carrying `result` (e.g. the error).
    pub fn failed(result: serde_json::Value) -> Self {
        Self {
            status: JobStatus::Failed,
            result,
        }
    }
}

/// Executes one claimed task. Implementations are shared across all workers, so
/// they must be `Send + Sync`.
pub trait JobRunner: Send + Sync {
    /// Run `task` to a terminal outcome.
    fn run(&self, task: &DaemonTaskRow) -> JobOutcome;
}

/// Adapts a closure into a [`JobRunner`] (handy for tests + simple runners).
pub struct FnJobRunner<F>(pub F);

impl<F> JobRunner for FnJobRunner<F>
where
    F: Fn(&DaemonTaskRow) -> JobOutcome + Send + Sync,
{
    fn run(&self, task: &DaemonTaskRow) -> JobOutcome {
        (self.0)(task)
    }
}

/// A [`JobRunner`] that executes each task as a live `jekko run --ephemeral --json`
/// subprocess (real model via the configured provider + live tools), reading the
/// prompt from `task.body_json["prompt"]` and capturing the JSON result.
///
/// No per-job wall-clock timeout yet: a hung job's lease simply expires and another
/// worker reclaims it. A hard timeout (kill on deadline) is a planned follow-up.
pub struct JekkoRunJobRunner {
    /// Path to the `jekko` binary (typically `std::env::current_exe()`).
    pub jekko_bin: PathBuf,
    /// Working directory for the subprocess.
    pub cwd: PathBuf,
    /// Provider passed to `jekko run --provider` (e.g. `jnoccio`); `None` lets jekko choose.
    pub provider: Option<String>,
    /// Extra args appended before the prompt.
    pub extra_args: Vec<String>,
}

impl JobRunner for JekkoRunJobRunner {
    fn run(&self, task: &DaemonTaskRow) -> JobOutcome {
        let prompt = task
            .body_json
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if prompt.is_empty() {
            return JobOutcome::failed(serde_json::json!({ "error": "task body missing 'prompt'" }));
        }

        let mut cmd = std::process::Command::new(&self.jekko_bin);
        cmd.current_dir(&self.cwd)
            .arg("run")
            .arg("--ephemeral")
            .arg("--json");
        if let Some(provider) = &self.provider {
            cmd.arg("--provider").arg(provider);
        }
        for arg in &self.extra_args {
            cmd.arg(arg);
        }
        cmd.arg(prompt);

        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
                    .unwrap_or_else(|_| serde_json::json!({ "raw_stdout": stdout }));
                let success = parsed
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or_else(|| output.status.success());
                if success {
                    JobOutcome::done(parsed)
                } else {
                    JobOutcome::failed(parsed)
                }
            }
            Err(err) => JobOutcome::failed(serde_json::json!({ "error": err.to_string() })),
        }
    }
}

/// Configuration for a bounded queue drain.
#[derive(Debug, Clone)]
pub struct BoundedQueueConfig {
    /// On-disk SQLite path (WAL) shared by all workers. A `:memory:` DB is
    /// single-connection only and cannot be shared across worker threads.
    pub db_path: PathBuf,
    /// Run scope (FK `daemon_task.run_id`).
    pub run_id: String,
    /// HARD cap: exactly this many worker threads are spawned. Clamped to `>= 1`.
    pub max_workers: usize,
    /// Lease duration (ms). A task not completed/heartbeated within this window
    /// becomes reclaimable (crash recovery).
    pub lease_ttl_ms: i64,
    /// Poll interval when there is no claimable work but tasks are still in flight.
    pub poll_interval: Duration,
    /// Attempts before a repeatedly-failing task is marked `failed` (not requeued).
    pub max_attempts: i64,
}

impl BoundedQueueConfig {
    /// Config with sensible defaults (60s lease, 50ms poll, 3 attempts).
    pub fn new(db_path: PathBuf, run_id: impl Into<String>, max_workers: usize) -> Self {
        Self {
            db_path,
            run_id: run_id.into(),
            max_workers: max_workers.max(1),
            lease_ttl_ms: 60_000,
            poll_interval: Duration::from_millis(50),
            max_attempts: 3,
        }
    }
}

/// Summary of a drain.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DrainReport {
    /// Tasks completed successfully.
    pub completed: usize,
    /// Tasks that exhausted `max_attempts` and were marked `failed`.
    pub failed: usize,
}

/// Current wall-clock in epoch-ms.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Count tasks still in the pipeline (`queued` + `ready` + `running`).
fn pending_count(db: &Db, run_id: &str) -> i64 {
    let conn = db.connection();
    ["queued", "ready", "running"]
        .iter()
        .map(|status| count_tasks_by_status(conn, run_id, status).unwrap_or(0))
        .sum()
}

/// Drain the queue to empty with a fixed pool of `config.max_workers` threads,
/// then return. Spawns EXACTLY `max_workers` threads and never more. Honors the
/// shared `cancel` flag for graceful shutdown — a cancelled worker finishes its
/// current task, then stops claiming new ones.
pub fn drain_to_empty(
    config: BoundedQueueConfig,
    runner: Arc<dyn JobRunner>,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<DrainReport> {
    let completed = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::with_capacity(config.max_workers);

    for index in 0..config.max_workers {
        let worker_id = format!("worker-{index}");
        let config = config.clone();
        let runner = Arc::clone(&runner);
        let cancel = Arc::clone(&cancel);
        let completed = Arc::clone(&completed);
        let failed = Arc::clone(&failed);
        let handle = std::thread::Builder::new()
            .name(worker_id.clone())
            .spawn(move || {
                worker_loop(&config, &worker_id, runner.as_ref(), &cancel, &completed, &failed);
            })?;
        handles.push(handle);
    }

    // A panicked worker must not poison the whole drain; just join best-effort.
    for handle in handles {
        let _ = handle.join();
    }

    Ok(DrainReport {
        completed: completed.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
    })
}

/// One worker's claim → run → complete loop. Owns its own DB connection.
fn worker_loop(
    config: &BoundedQueueConfig,
    worker_id: &str,
    runner: &dyn JobRunner,
    cancel: &AtomicBool,
    completed: &AtomicUsize,
    failed: &AtomicUsize,
) {
    // Each worker opens its own connection to the shared on-disk DB; WAL +
    // busy_timeout (applied by `Db::open`) handle concurrent access.
    let db = match Db::open(&config.db_path) {
        Ok(db) => db,
        Err(_) => return,
    };

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let claimed = claim_next_task(
            db.connection(),
            &config.run_id,
            worker_id,
            now_ms(),
            config.lease_ttl_ms,
        );

        match claimed {
            Ok(Some(task)) => {
                let outcome = runner.run(&task);
                let conn = db.connection();
                match outcome.status {
                    JobStatus::Done => {
                        let _ = complete_task(
                            conn,
                            &task.id,
                            worker_id,
                            "done",
                            Some(&outcome.result),
                            now_ms(),
                        );
                        completed.fetch_add(1, Ordering::Relaxed);
                    }
                    JobStatus::Failed => {
                        // `attempt_count` was already incremented by the claim.
                        if task.attempt_count >= config.max_attempts {
                            let _ = complete_task(
                                conn,
                                &task.id,
                                worker_id,
                                "failed",
                                Some(&outcome.result),
                                now_ms(),
                            );
                            failed.fetch_add(1, Ordering::Relaxed);
                        } else {
                            let _ = release_task(conn, &task.id, worker_id, "queued", now_ms());
                        }
                    }
                }
            }
            Ok(None) => {
                // Nothing claimable. If nothing is pending anywhere, we're drained.
                if pending_count(&db, &config.run_id) == 0 {
                    break;
                }
                // Other workers still hold tasks that may be requeued — wait, retry.
                std::thread::sleep(config.poll_interval);
            }
            Err(_) => std::thread::sleep(config.poll_interval),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use jekko_store::daemon::{get_task, upsert_task, DaemonTaskRow};

    /// Open a fresh on-disk DB and seed it with `n` queued tasks. FK enforcement
    /// is disabled on the seeding connection so we can insert tasks without the
    /// full session → daemon_run parent chain (workers never modify the FK column,
    /// so they run fine with FK enforcement on).
    fn seed_db(path: &std::path::Path, run_id: &str, n: usize) -> Db {
        let db = Db::open(path).expect("open db");
        db.connection()
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .expect("disable fk for seeding");
        for i in 0..n {
            upsert_task(db.connection(), &queued(run_id, &format!("t{i}"), 1)).unwrap();
        }
        db
    }

    fn queued(run_id: &str, id: &str, priority: i64) -> DaemonTaskRow {
        DaemonTaskRow {
            id: id.to_string(),
            run_id: run_id.to_string(),
            external_id: None,
            title: format!("task {id}"),
            body_json: serde_json::json!({ "prompt": "noop" }),
            status: "queued".to_string(),
            lane: "normal".to_string(),
            phase: "queued".to_string(),
            difficulty_score: 0.0,
            risk_score: 0.0,
            readiness_score: 0.0,
            implementation_confidence: 0.0,
            verification_confidence: 0.0,
            attempt_count: 0,
            no_progress_count: 0,
            incubator_round: 0,
            incubator_status: "none".to_string(),
            accepted_artifact_id: None,
            last_assessment_json: None,
            promotion_result_json: None,
            blocked_reason: None,
            priority,
            lease_worker_id: None,
            lease_expires_at: None,
            locked_paths_json: None,
            evidence_json: None,
            time_created: 1_000,
            time_updated: 1_000,
        }
    }

    #[test]
    fn drains_all_tasks_with_fixed_pool() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.db");
        let run_id = "run-1";
        let db = seed_db(&path, run_id, 12);

        let runner = Arc::new(FnJobRunner(|_t: &DaemonTaskRow| {
            JobOutcome::done(serde_json::json!({ "ok": true }))
        }));
        let config = BoundedQueueConfig::new(path.clone(), run_id, 4);
        let report =
            drain_to_empty(config, runner, Arc::new(AtomicBool::new(false))).unwrap();

        assert_eq!(report.completed, 12);
        assert_eq!(report.failed, 0);
        assert_eq!(pending_count(&db, run_id), 0);
        for i in 0..12 {
            let t = get_task(db.connection(), &format!("t{i}")).unwrap().unwrap();
            assert_eq!(t.status, "done");
        }
    }

    #[test]
    fn never_exceeds_worker_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.db");
        let run_id = "run-cap";
        let _db = seed_db(&path, run_id, 24);

        let live = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let live_c = Arc::clone(&live);
        let max_c = Arc::clone(&max_seen);

        let cap = 3usize;
        let runner = Arc::new(FnJobRunner(move |_t: &DaemonTaskRow| {
            let now = live_c.fetch_add(1, Ordering::SeqCst) + 1;
            max_c.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(5)); // force overlap
            live_c.fetch_sub(1, Ordering::SeqCst);
            JobOutcome::done(serde_json::json!({}))
        }));
        let config = BoundedQueueConfig::new(path, run_id, cap);
        let report =
            drain_to_empty(config, runner, Arc::new(AtomicBool::new(false))).unwrap();

        assert_eq!(report.completed, 24);
        let observed = max_seen.load(Ordering::SeqCst);
        assert!(observed <= cap, "observed {observed} workers > cap {cap}");
        assert!(observed >= 2, "expected real parallelism, observed {observed}");
    }

    #[test]
    fn retries_failed_task_then_completes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.db");
        let run_id = "run-retry";
        let db = seed_db(&path, run_id, 1);

        // Fail the first two attempts of t0, succeed on the third.
        let calls: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
        let calls_c = Arc::clone(&calls);
        let runner = Arc::new(FnJobRunner(move |t: &DaemonTaskRow| {
            let mut map = calls_c.lock().unwrap();
            let n = map.entry(t.id.clone()).or_insert(0);
            *n += 1;
            if *n < 3 {
                JobOutcome::failed(serde_json::json!({ "attempt": *n }))
            } else {
                JobOutcome::done(serde_json::json!({ "attempt": *n }))
            }
        }));
        let mut config = BoundedQueueConfig::new(path, run_id, 2);
        config.max_attempts = 5;
        let report =
            drain_to_empty(config, runner, Arc::new(AtomicBool::new(false))).unwrap();

        assert_eq!(report.completed, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(*calls.lock().unwrap().get("t0").unwrap(), 3);
        let t = get_task(db.connection(), "t0").unwrap().unwrap();
        assert_eq!(t.status, "done");
        assert_eq!(t.attempt_count, 3);
    }
}
