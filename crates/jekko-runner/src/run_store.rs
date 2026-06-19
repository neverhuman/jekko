//! Durable run store (F6).
//!
//! Makes a run **checkpointable, resumable, replayable, and forkable** on top of
//! the append-only NDJSON event log ([`crate::events`]):
//!
//!   * [`RunEventStore`] — the storage trait: append → offset, read_from,
//!     snapshot, latest_snapshot.
//!   * [`NdjsonRunStore`] — the on-disk impl over
//!     `target/zyal/runs/<run_id>/{events.jsonl, snapshots/}`.
//!   * [`RunSnapshot`] — the foldable run state at an offset (watcher metrics,
//!     budget, open leases, taint-violation count), so resume folds the tail
//!     onto the latest snapshot instead of the whole log.
//!   * [`SourceMode`] / [`RunContext`] — `Replay` reconstructs from the durable
//!     log and **forbids adapter `invoke()`** (no network, no secrets); `Fake`
//!     is the deterministic CI mode; `Live` is normal execution.
//!   * [`checkpoint`] / [`resume`] / [`replay`] / [`fork`] operations.
//!
//! Determinism: snapshots embed only folded scalars (no wall-clock beyond the
//! event `ts` already in the log), so a replay reconstructs byte-identical state.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::events::{Event, EventKind, RUNS_DIR_REL};
use crate::watcher::WatcherSnapshot;

/// How a run is being executed. `Replay` must never touch the network or
/// secrets — it reconstructs from the durable log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    /// Normal execution: real adapters, subject to the trust pipeline.
    #[default]
    Live,
    /// Reconstruct from the durable log; adapters MUST NOT `invoke()`.
    Replay,
    /// Deterministic CI: adapters return canned output, no network.
    Fake,
}

impl SourceMode {
    /// Whether a tool adapter may perform a live `invoke()` in this mode.
    /// Replay reconstructs outputs from receipts, so live invocation is barred.
    pub fn allows_invoke(self) -> bool {
        !matches!(self, SourceMode::Replay)
    }
}

/// The execution context threaded into the kernel so it can gate side effects.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub run_id: String,
    pub mode: SourceMode,
}

impl RunContext {
    pub fn live(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            mode: SourceMode::Live,
        }
    }
    pub fn replay(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            mode: SourceMode::Replay,
        }
    }
    pub fn fake(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            mode: SourceMode::Fake,
        }
    }
}

/// Foldable run state at a given offset. Resume folds the post-snapshot tail
/// onto this rather than replaying the whole log.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunSnapshot {
    pub run_id: String,
    /// The store contains events `[0, at_offset)`; resume reads from here.
    pub at_offset: u64,
    pub watcher: WatcherSnapshot,
    pub budget_spent_usd: f64,
    /// Lease ids granted in this window (redacted handles only). Release
    /// tracking (a `LeaseReleased` event subtracted here) lands with the live
    /// runner wiring; for now this is "all leases granted", not "currently open".
    pub open_leases: Vec<String>,
    pub taint_violations: u64,
}

/// Fold a slice of events into a [`RunSnapshot`] (pure — no I/O). The single
/// source of truth for "the run's state at the end of these events".
pub fn fold_run_snapshot(run_id: &str, events: &[Event]) -> RunSnapshot {
    let watcher = crate::watcher::fold_events(events);
    let mut open_leases = Vec::new();
    let mut taint_violations = 0u64;
    for e in events {
        match e.kind {
            EventKind::LeaseGranted => {
                if let Some(id) = e.data.get("lease_id").and_then(|v| v.as_str()) {
                    open_leases.push(id.to_string());
                }
            }
            EventKind::TaintViolation => taint_violations += 1,
            _ => {}
        }
    }
    let at_offset = events.iter().map(|e| e.event_offset + 1).max().unwrap_or(0);
    RunSnapshot {
        run_id: run_id.to_string(),
        at_offset,
        budget_spent_usd: watcher.model_spend_usd,
        watcher,
        open_leases,
        taint_violations,
    }
}

/// Durable storage for a run's events + periodic snapshots.
pub trait RunEventStore {
    /// Append `event` to the durable log, returning its stored offset.
    fn append(&self, event: &Event) -> Result<u64>;
    /// Read every event with `event_offset >= offset`, in order.
    fn read_from(&self, offset: u64) -> Result<Vec<Event>>;
    /// Persist a run snapshot (durably, at its `at_offset`).
    fn snapshot(&self, snap: &RunSnapshot) -> Result<()>;
    /// The highest-offset snapshot, if any.
    fn latest_snapshot(&self) -> Result<Option<RunSnapshot>>;
}

/// The result of [`resume`]: the snapshot to fold from (if any) plus the tail of
/// events after it.
#[derive(Debug, Clone)]
pub struct Resumed {
    pub snapshot: Option<RunSnapshot>,
    pub tail: Vec<Event>,
}

/// On-disk store over `target/zyal/runs/<run_id>/{events.jsonl, snapshots/}`.
pub struct NdjsonRunStore {
    repo_root: PathBuf,
    run_id: String,
}

impl NdjsonRunStore {
    pub fn new(repo_root: impl Into<PathBuf>, run_id: impl Into<String>) -> Self {
        Self {
            repo_root: repo_root.into(),
            run_id: run_id.into(),
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    fn events_path(&self) -> PathBuf {
        self.repo_root
            .join(crate::events::run_event_file_rel(&self.run_id))
    }

    fn snapshots_dir(&self) -> PathBuf {
        self.repo_root
            .join(RUNS_DIR_REL)
            .join(&self.run_id)
            .join("snapshots")
    }
}

impl RunEventStore for NdjsonRunStore {
    fn append(&self, event: &Event) -> Result<u64> {
        let path = self.events_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir -p {}", parent.display()))?;
        }
        let line = serde_json::to_string(event).context("serialize event")?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        writeln!(file, "{line}").with_context(|| format!("append {}", path.display()))?;
        Ok(event.event_offset)
    }

    fn read_from(&self, offset: u64) -> Result<Vec<Event>> {
        let path = self.events_path();
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
        };
        let mut out = Vec::new();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Skip an unparseable line (e.g. a crash-truncated trailing write)
            // rather than bricking resume/replay/fork — best-effort recovery over
            // a corrupt tail. Each line is ≤512 B and newline-framed, so the
            // blast radius of a torn write is a single event.
            let Ok(event) = serde_json::from_str::<Event>(line) else {
                continue;
            };
            if event.event_offset >= offset {
                out.push(event);
            }
        }
        Ok(out)
    }

    fn snapshot(&self, snap: &RunSnapshot) -> Result<()> {
        let dir = self.snapshots_dir();
        fs::create_dir_all(&dir).with_context(|| format!("mkdir -p {}", dir.display()))?;
        // Zero-pad the offset so a lexicographic scan yields snapshots in order.
        let final_path = dir.join(format!("{:020}.json", snap.at_offset));
        // Unique tmp name per write (pid + monotonic seq) so two concurrent
        // snapshots at the SAME offset can't clobber each other's temp file
        // before the atomic rename.
        static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp_path = dir.join(format!(
            "{:020}.json.{}.{}.tmp",
            snap.at_offset,
            std::process::id(),
            TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let body = serde_json::to_string_pretty(snap).context("serialize snapshot")?;
        fs::write(&tmp_path, body).with_context(|| format!("write {}", tmp_path.display()))?;
        // Atomic publish: a partially-written snapshot never becomes the latest.
        fs::rename(&tmp_path, &final_path).with_context(|| {
            format!("rename {} -> {}", tmp_path.display(), final_path.display())
        })?;
        Ok(())
    }

    fn latest_snapshot(&self) -> Result<Option<RunSnapshot>> {
        let dir = self.snapshots_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("read dir {}", dir.display())),
        };
        let mut best: Option<(u64, PathBuf)> = None;
        for entry in entries {
            let path = entry?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let Some(stem) = name.strip_suffix(".json") else {
                continue;
            };
            let Ok(offset) = stem.parse::<u64>() else {
                continue;
            };
            if best.as_ref().is_none_or(|(b, _)| offset > *b) {
                best = Some((offset, path));
            }
        }
        let Some((_, path)) = best else {
            return Ok(None);
        };
        let body = match fs::read_to_string(&path) {
            Ok(b) => b,
            // The chosen snapshot was deleted between the dir listing and the
            // read (rotation/cleanup): fall back to "no snapshot" gracefully.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
        };
        let snap: RunSnapshot = serde_json::from_str(&body)
            .with_context(|| format!("parse snapshot {}", path.display()))?;
        Ok(Some(snap))
    }
}

/// Write a checkpoint: persist the folded snapshot at the current offset.
pub fn checkpoint(store: &dyn RunEventStore, snap: &RunSnapshot) -> Result<()> {
    store.snapshot(snap)
}

/// Load the latest snapshot plus the tail of events after it, ready to fold
/// forward. With no snapshot, the tail is the whole log.
pub fn resume(store: &dyn RunEventStore) -> Result<Resumed> {
    let snapshot = store.latest_snapshot()?;
    let from = snapshot.as_ref().map(|s| s.at_offset).unwrap_or(0);
    let tail = store.read_from(from)?;
    Ok(Resumed { snapshot, tail })
}

/// Replay reconstructs full run state from offset 0 in [`SourceMode::Replay`]
/// (adapters are barred from live `invoke()`). Returns the replay context plus
/// the full event stream the caller folds.
pub fn replay(store: &dyn RunEventStore, run_id: &str) -> Result<(RunContext, Vec<Event>)> {
    Ok((RunContext::replay(run_id), store.read_from(0)?))
}

/// Fork: copy the event prefix `[0, at_offset)` from `source` into `dest`,
/// returning the number of events copied. The caller emits `ForkCreated`.
pub fn fork(source: &dyn RunEventStore, dest: &dyn RunEventStore, at_offset: u64) -> Result<usize> {
    let prefix: Vec<Event> = source
        .read_from(0)?
        .into_iter()
        .filter(|e| e.event_offset < at_offset)
        .collect();
    for e in &prefix {
        dest.append(e)?;
    }
    Ok(prefix.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(offset: u64, kind: EventKind, data: serde_json::Value) -> Event {
        Event {
            ts: 100 + offset,
            kind,
            run_id: "r1".into(),
            event_offset: offset,
            data,
        }
    }

    /// A per-test scratch dir that auto-removes on drop (survives panics).
    /// Unique `name` keeps parallel tests isolated; it pre-deletes on creation
    /// so a same-pid re-run starts clean.
    struct TmpDir(PathBuf);
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    impl AsRef<std::path::Path> for TmpDir {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl From<&TmpDir> for PathBuf {
        fn from(t: &TmpDir) -> Self {
            t.0.clone()
        }
    }
    fn tmp(name: &str) -> TmpDir {
        let mut p = std::env::temp_dir();
        p.push(format!("jekko-runstore-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        TmpDir(p)
    }

    #[test]
    fn source_mode_replay_forbids_invoke() {
        assert!(SourceMode::Live.allows_invoke());
        assert!(SourceMode::Fake.allows_invoke());
        assert!(!SourceMode::Replay.allows_invoke());
    }

    #[test]
    fn append_then_read_from_round_trips_by_offset() {
        let root = tmp("append");
        let store = NdjsonRunStore::new(&root, "r1");
        for o in 0..5 {
            store
                .append(&ev(o, EventKind::WorkerStarted, json!({})))
                .unwrap();
        }
        let all = store.read_from(0).unwrap();
        assert_eq!(all.len(), 5);
        let tail = store.read_from(3).unwrap();
        assert_eq!(
            tail.iter().map(|e| e.event_offset).collect::<Vec<_>>(),
            vec![3, 4]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn snapshot_and_latest_snapshot_round_trip() {
        let root = tmp("snapshot");
        let store = NdjsonRunStore::new(&root, "r1");
        store
            .snapshot(&RunSnapshot {
                run_id: "r1".into(),
                at_offset: 4,
                ..Default::default()
            })
            .unwrap();
        store
            .snapshot(&RunSnapshot {
                run_id: "r1".into(),
                at_offset: 9,
                ..Default::default()
            })
            .unwrap();
        let latest = store.latest_snapshot().unwrap().unwrap();
        assert_eq!(latest.at_offset, 9); // highest offset wins
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fold_run_snapshot_counts_leases_and_taint() {
        let events = vec![
            ev(0, EventKind::WorkerStarted, json!({})),
            ev(
                1,
                EventKind::LeaseGranted,
                json!({"lease_id": "L1", "tool_id": "t", "node_id": "n"}),
            ),
            ev(
                2,
                EventKind::TaintViolation,
                json!({"node_id": "n", "action": "shell"}),
            ),
            ev(3, EventKind::LeaseGranted, json!({"lease_id": "L2"})),
        ];
        let snap = fold_run_snapshot("r1", &events);
        assert_eq!(snap.at_offset, 4);
        assert_eq!(snap.open_leases, vec!["L1", "L2"]);
        assert_eq!(snap.taint_violations, 1);
    }

    #[test]
    fn resume_returns_snapshot_plus_tail() {
        let root = tmp("resume");
        let store = NdjsonRunStore::new(&root, "r1");
        for o in 0..6 {
            store
                .append(&ev(o, EventKind::WorkerStarted, json!({})))
                .unwrap();
        }
        store
            .snapshot(&RunSnapshot {
                run_id: "r1".into(),
                at_offset: 4,
                ..Default::default()
            })
            .unwrap();
        let resumed = resume(&store).unwrap();
        assert_eq!(resumed.snapshot.unwrap().at_offset, 4);
        assert_eq!(
            resumed
                .tail
                .iter()
                .map(|e| e.event_offset)
                .collect::<Vec<_>>(),
            vec![4, 5]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fork_copies_only_the_prefix() {
        let root = tmp("fork");
        let source = NdjsonRunStore::new(&root, "r1");
        for o in 0..6 {
            source
                .append(&ev(o, EventKind::WorkerStarted, json!({})))
                .unwrap();
        }
        let dest = NdjsonRunStore::new(&root, "r1-fork");
        let copied = fork(&source, &dest, 4).unwrap();
        assert_eq!(copied, 4);
        let forked = dest.read_from(0).unwrap();
        assert_eq!(
            forked.iter().map(|e| e.event_offset).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn replay_reads_full_stream_in_replay_mode() {
        let root = tmp("replay");
        let store = NdjsonRunStore::new(&root, "r1");
        for o in 0..3 {
            store
                .append(&ev(o, EventKind::WorkerStarted, json!({})))
                .unwrap();
        }
        let (ctx, events) = replay(&store, "r1").unwrap();
        assert_eq!(ctx.mode, SourceMode::Replay);
        assert!(!ctx.mode.allows_invoke());
        assert_eq!(events.len(), 3);
        let _ = fs::remove_dir_all(&root);
    }
}
