//! `jekko zyal-queue` — a durable, bounded job queue.
//!
//! Submit jobs (each carrying a prompt) into a SQLite-backed queue and drain them
//! with a FIXED pool of N workers that each run `jekko run` live (real model +
//! tools). Many producers can enqueue concurrently; the bounded pool reliably
//! completes every job, **never spawns more than N workers**, and recovers tasks
//! orphaned by a crashed worker (expired-lease reclaim). All Rust, all durable.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Args;

use jekko_runner::bounded_queue::{drain_to_empty, BoundedQueueConfig, JekkoRunJobRunner};
use jekko_store::daemon::{
    count_tasks_by_status, ensure_placeholder_run, get_task, queued_task, upsert_task,
};
use jekko_store::Db;

use crate::cli::GlobalOpts;

/// `jekko zyal-queue` arguments.
#[derive(Args, Debug)]
pub struct ZyalQueueArgs {
    /// SQLite queue DB path (created if missing).
    #[arg(long, value_name = "PATH")]
    pub db: PathBuf,

    /// Run scope grouping the submitted jobs.
    #[arg(long = "run-id", value_name = "ID", default_value = "zyal-queue")]
    pub run_id: String,

    /// HARD cap: number of worker threads (never exceeded).
    #[arg(long, default_value_t = 4)]
    pub workers: usize,

    /// JSON file with an array of jobs: `[{"id"?, "prompt", "priority"?}, ...]`.
    #[arg(long, value_name = "PATH")]
    pub jobs: Option<PathBuf>,

    /// Submit an inline job with this prompt (repeatable; combinable with --jobs).
    #[arg(long, value_name = "TEXT")]
    pub prompt: Vec<String>,

    /// Enqueue only; do not drain.
    #[arg(long = "no-drain", action = clap::ArgAction::SetTrue)]
    pub no_drain: bool,

    /// Provider for the live `jekko run` workers.
    #[arg(long, default_value = "jnoccio")]
    pub provider: String,

    /// Lease TTL (seconds) before an un-heartbeated task is reclaimable.
    #[arg(long = "lease-ttl-secs", default_value_t = 120)]
    pub lease_ttl_secs: u64,

    /// Directory to write each job's result JSON to (`<dir>/<task-id>.json`), so a
    /// downstream assembler can collect outputs. Created if missing.
    #[arg(long = "results-dir", value_name = "DIR")]
    pub results_dir: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
struct JobSpec {
    id: Option<String>,
    prompt: String,
    priority: Option<i64>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn run(_global: &GlobalOpts, args: &ZyalQueueArgs) -> Result<()> {
    let db = Db::open(&args.db).with_context(|| format!("open queue db {}", args.db.display()))?;
    // Standalone job queue: we don't bootstrap the full session/daemon_run chain,
    // so relax FK enforcement on this submission connection and ensure a parent run
    // row exists for the tasks' foreign key. (Workers update non-FK columns only, so
    // they run fine with FK enforcement on.)
    db.connection()
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .context("relax fk for queue submission")?;
    ensure_placeholder_run(db.connection(), &args.run_id, now_ms())
        .context("ensure parent run row")?;

    let jobs = load_jobs(args)?;
    let now = now_ms();
    let mut ids = Vec::with_capacity(jobs.len());
    for (i, job) in jobs.iter().enumerate() {
        let id = job
            .id
            .clone()
            .unwrap_or_else(|| format!("{}-{}-{}", args.run_id, now, i));
        let title: String = job.prompt.chars().take(80).collect();
        let row = queued_task(
            &args.run_id,
            &id,
            title,
            serde_json::json!({ "prompt": job.prompt }),
            job.priority.unwrap_or(1),
            now,
        );
        upsert_task(db.connection(), &row).with_context(|| format!("enqueue job {id}"))?;
        ids.push(id);
    }
    println!(
        "zyal-queue: enqueued {} job(s) into run '{}' ({})",
        jobs.len(),
        args.run_id,
        args.db.display()
    );

    if args.no_drain {
        let queued = count_tasks_by_status(db.connection(), &args.run_id, "queued")?;
        println!("zyal-queue: --no-drain set; {queued} job(s) queued, not drained.");
        return Ok(());
    }

    let runner = JekkoRunJobRunner {
        jekko_bin: std::env::current_exe().context("resolve jekko binary")?,
        cwd: std::env::current_dir().context("resolve cwd")?,
        provider: Some(args.provider.clone()),
        extra_args: Vec::new(),
    };
    let mut config = BoundedQueueConfig::new(args.db.clone(), args.run_id.clone(), args.workers);
    config.lease_ttl_ms = (args.lease_ttl_secs as i64) * 1000;
    config.poll_interval = Duration::from_millis(100);

    println!(
        "zyal-queue: draining with {} worker(s) [hard cap] via live `jekko run --provider {}`…",
        config.max_workers, args.provider
    );
    let report = drain_to_empty(config, Arc::new(runner), Arc::new(AtomicBool::new(false)))?;
    println!(
        "zyal-queue: done — {} completed, {} failed",
        report.completed, report.failed
    );

    if let Some(dir) = &args.results_dir {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create results dir {}", dir.display()))?;
    }
    let mut written = 0usize;
    for id in &ids {
        if let Some(task) = get_task(db.connection(), id)? {
            println!("  [{}] {}", task.status, task.id);
            if let Some(dir) = &args.results_dir {
                let result = task
                    .promotion_result_json
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({ "status": task.status }));
                let path = dir.join(format!("{id}.json"));
                std::fs::write(&path, serde_json::to_vec_pretty(&result)?)
                    .with_context(|| format!("write result {}", path.display()))?;
                written += 1;
            }
        }
    }
    if let Some(dir) = &args.results_dir {
        println!("zyal-queue: wrote {written} result file(s) to {}", dir.display());
    }
    Ok(())
}

fn load_jobs(args: &ZyalQueueArgs) -> Result<Vec<JobSpec>> {
    let mut jobs: Vec<JobSpec> = Vec::new();
    if let Some(path) = &args.jobs {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read jobs file {}", path.display()))?;
        let parsed: Vec<JobSpec> =
            serde_json::from_str(&text).context("parse jobs JSON (expected an array)")?;
        jobs.extend(parsed);
    }
    for prompt in &args.prompt {
        jobs.push(JobSpec {
            id: None,
            prompt: prompt.clone(),
            priority: None,
        });
    }
    if jobs.is_empty() {
        anyhow::bail!("no jobs to submit: pass --jobs <file> and/or --prompt <text>");
    }
    Ok(jobs)
}
