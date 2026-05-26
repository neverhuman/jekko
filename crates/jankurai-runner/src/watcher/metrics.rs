//! Pure aggregate over a slice of [`Event`]s — no I/O, no DB, fully testable.

use std::collections::BTreeMap;

use crate::events::{Event, EventKind};

/// Snapshot of progress + health for one ZYAL run (or one mega-run stage).
///
/// Built by folding the event stream from earliest to latest; later events
/// override earlier values for `last_*` fields. All counters monotonic.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WatcherSnapshot {
    pub lanes_started: u64,
    pub lanes_finished: u64,
    pub workers_pass: u64,
    pub workers_fail: u64,
    pub parity_gaps_open: i64,
    pub parity_gaps_closed: u64,
    pub model_attempts: u64,
    pub model_failures: u64,
    pub errors_by_provider: BTreeMap<String, u64>,
    pub model_spend_usd: f64,
    /// Last `ts` we saw for any progress-class event (anything except
    /// `Heartbeat`). Used by [`crate::watcher::remediation::detect_and_remediate`]
    /// to detect stalls.
    pub last_progress_ts: Option<u64>,
    /// Last reported `data.score` from a jankurai score event. None until a
    /// score event is seen.
    pub last_jankurai_score: Option<i64>,
    /// Last reported `data.hard_findings` count. None until seen.
    pub last_jankurai_hard_findings: Option<i64>,
    /// Run is marked finished once we see a [`EventKind::RunFinished`].
    pub finished: bool,
}

impl WatcherSnapshot {
    /// Cumulative throughput in lanes per minute over the window between the
    /// first observed event timestamp and `now_ts`. Returns 0.0 when no
    /// elapsed time can be computed.
    pub fn lanes_per_minute(&self, first_ts: Option<u64>, now_ts: u64) -> f64 {
        match (first_ts, self.lanes_finished) {
            (Some(start), finished) if now_ts > start && finished > 0 => {
                let minutes = (now_ts - start) as f64 / 60.0;
                if minutes > 0.0 {
                    finished as f64 / minutes
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    /// Provider error rate (failures / attempts) clipped to `[0.0, 1.0]`.
    pub fn error_rate(&self) -> f64 {
        if self.model_attempts == 0 {
            0.0
        } else {
            (self.model_failures as f64 / self.model_attempts as f64).clamp(0.0, 1.0)
        }
    }
}

/// Fold a slice of events into a [`WatcherSnapshot`]. Events should be in
/// timestamp order; we don't sort here.
pub fn fold_events(events: &[Event]) -> WatcherSnapshot {
    let mut snap = WatcherSnapshot::default();
    for event in events {
        if !matches!(event.kind, EventKind::Heartbeat) {
            snap.last_progress_ts = Some(event.ts);
        }
        match event.kind {
            EventKind::ReasoningLane => {
                // ReasoningLane events fire on start AND finish; treat
                // `data.status == "complete"` as a finish, else start.
                if event.data.get("status").and_then(|s| s.as_str()) == Some("complete") {
                    snap.lanes_finished = snap.lanes_finished.saturating_add(1);
                } else {
                    snap.lanes_started = snap.lanes_started.saturating_add(1);
                }
            }
            EventKind::WorkerStarted => {
                snap.lanes_started = snap.lanes_started.saturating_add(1);
            }
            EventKind::WorkerPass => {
                snap.workers_pass = snap.workers_pass.saturating_add(1);
            }
            EventKind::WorkerFail => {
                snap.workers_fail = snap.workers_fail.saturating_add(1);
            }
            EventKind::ParityGap => {
                snap.parity_gaps_open = snap.parity_gaps_open.saturating_add(1);
            }
            EventKind::ParityResult => {
                if let Some(closed) = event.data.get("gaps_closed").and_then(|v| v.as_i64()) {
                    snap.parity_gaps_closed =
                        snap.parity_gaps_closed.saturating_add(closed.max(0) as u64);
                    snap.parity_gaps_open = snap.parity_gaps_open.saturating_sub(closed.max(0));
                }
            }
            EventKind::ModelAttempt => {
                snap.model_attempts = snap.model_attempts.saturating_add(1);
            }
            EventKind::ModelOutcome | EventKind::ModelAttemptOutcome => {
                let state = event.data.get("state").and_then(|v| v.as_str());
                let success = matches!(state, Some("parsed"));
                if !success {
                    snap.model_failures = snap.model_failures.saturating_add(1);
                    if let Some(provider) = event.data.get("provider").and_then(|v| v.as_str()) {
                        *snap
                            .errors_by_provider
                            .entry(provider.to_string())
                            .or_insert(0) += 1;
                    }
                }
                if let Some(cost) = event.data.get("cost_usd").and_then(|v| v.as_f64()) {
                    if cost.is_finite() {
                        snap.model_spend_usd += cost.max(0.0);
                    }
                }
            }
            EventKind::AuditResult => {
                if let Some(score) = event.data.get("score").and_then(|v| v.as_i64()) {
                    snap.last_jankurai_score = Some(score);
                }
                if let Some(hard) = event.data.get("hard_findings").and_then(|v| v.as_i64()) {
                    snap.last_jankurai_hard_findings = Some(hard);
                }
            }
            EventKind::RunFinished => {
                snap.finished = true;
            }
            _ => {}
        }
    }
    snap
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(kind: EventKind, ts: u64, data: serde_json::Value) -> Event {
        Event {
            ts,
            kind,
            run_id: "r1".into(),
            data,
        }
    }

    #[test]
    fn empty_stream_yields_empty_snapshot() {
        let snap = fold_events(&[]);
        assert_eq!(snap, WatcherSnapshot::default());
    }

    #[test]
    fn counts_lanes_started_and_finished() {
        let events = vec![
            ev(
                EventKind::ReasoningLane,
                1,
                json!({"id": "lane-1", "status": "started"}),
            ),
            ev(
                EventKind::ReasoningLane,
                2,
                json!({"id": "lane-1", "status": "complete"}),
            ),
            ev(
                EventKind::ReasoningLane,
                3,
                json!({"id": "lane-2", "status": "complete"}),
            ),
        ];
        let snap = fold_events(&events);
        assert_eq!(snap.lanes_started, 1);
        assert_eq!(snap.lanes_finished, 2);
    }

    #[test]
    fn tracks_model_attempts_failures_and_spend() {
        let events = vec![
            ev(
                EventKind::ModelAttempt,
                1,
                json!({"kind": "frame", "attempt": 1}),
            ),
            ev(
                EventKind::ModelAttemptOutcome,
                2,
                json!({"state": "retryable_failure", "provider": "openrouter", "cost_usd": 0.0}),
            ),
            ev(
                EventKind::ModelAttempt,
                3,
                json!({"kind": "frame", "attempt": 2}),
            ),
            ev(
                EventKind::ModelOutcome,
                4,
                json!({"state": "parsed", "provider": "openrouter", "cost_usd": 0.0123}),
            ),
        ];
        let snap = fold_events(&events);
        assert_eq!(snap.model_attempts, 2);
        assert_eq!(snap.model_failures, 1);
        assert_eq!(snap.errors_by_provider.get("openrouter").copied(), Some(1));
        assert!((snap.model_spend_usd - 0.0123).abs() < 1e-9);
        assert!((snap.error_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn parity_gaps_open_decreases_when_results_close_them() {
        let events = vec![
            ev(EventKind::ParityGap, 1, json!({"id": "g1"})),
            ev(EventKind::ParityGap, 2, json!({"id": "g2"})),
            ev(EventKind::ParityGap, 3, json!({"id": "g3"})),
            ev(EventKind::ParityResult, 4, json!({"gaps_closed": 2})),
        ];
        let snap = fold_events(&events);
        assert_eq!(snap.parity_gaps_open, 1);
        assert_eq!(snap.parity_gaps_closed, 2);
    }

    #[test]
    fn last_progress_ts_skips_heartbeats() {
        let events = vec![
            ev(EventKind::WorkerStarted, 10, json!({"worker": "w-1"})),
            ev(EventKind::Heartbeat, 20, json!({})),
            ev(EventKind::Heartbeat, 30, json!({})),
        ];
        let snap = fold_events(&events);
        assert_eq!(snap.last_progress_ts, Some(10));
    }

    #[test]
    fn jankurai_score_and_hard_findings_tracked() {
        let events = vec![ev(
            EventKind::AuditResult,
            1,
            json!({"score": 88, "hard_findings": 0}),
        )];
        let snap = fold_events(&events);
        assert_eq!(snap.last_jankurai_score, Some(88));
        assert_eq!(snap.last_jankurai_hard_findings, Some(0));
    }

    #[test]
    fn run_finished_flag_set_on_terminator() {
        let events = vec![
            ev(EventKind::WorkerStarted, 1, json!({})),
            ev(EventKind::RunFinished, 2, json!({})),
        ];
        let snap = fold_events(&events);
        assert!(snap.finished);
    }

    #[test]
    fn lanes_per_minute_computes_throughput() {
        let mut snap = WatcherSnapshot::default();
        snap.lanes_finished = 6;
        // 6 lanes in 120s = 3 / min
        assert!((snap.lanes_per_minute(Some(0), 120) - 3.0).abs() < 1e-9);
        // 0 lanes => 0/min
        snap.lanes_finished = 0;
        assert_eq!(snap.lanes_per_minute(Some(0), 120), 0.0);
        // Missing start ts => 0/min
        snap.lanes_finished = 5;
        assert_eq!(snap.lanes_per_minute(None, 120), 0.0);
    }
}
