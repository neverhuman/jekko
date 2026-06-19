//! Detect consecutive empty-response model attempts at the same task kind.
//!
//! Post-FIX-1 the runner correctly classifies a subprocess that exits 0 +
//! writes `success: true` JSON with `response_bytes: 0` as a real success
//! at the syscall layer but a `retryable_failure` at the parse layer. That
//! merges two genuinely different failure modes:
//!
//! - The model returned content the parser doesn't like (JSON shape drift).
//! - The model literally returned 0 bytes (declined the request, hit an
//!   output budget, tripped a content filter).
//!
//! This tracker watches the receipt stream for streaks of the empty-byte
//! variant and emits one `EventKind::EmptyResponseStreak` event when the
//! count crosses a threshold. The signal is distinct, so operators reading
//! a SUMMARY.json (Phase F) can immediately recommend `quality_band: top20`
//! on the affected stage rather than fishing through retryable_failures.

use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::{json, Value};

use crate::events::{EventKind, EventSink};
use crate::model_client::ModelCallReceipt;

/// Number of consecutive empty receipts that constitute a streak.
pub const STREAK_THRESHOLD: usize = 3;

/// Tracks consecutive `response_bytes == 0` receipts for a single task kind.
///
/// Construct a fresh tracker per `(run, kind)` retry loop. Call
/// `record(&receipt, sink)` after every receipt; the tracker resets on a
/// non-empty receipt and emits at most one `EmptyResponseStreak` event per
/// streak (deduplicates further empty receipts within the same streak).
pub struct EmptyResponseTracker {
    kind: String,
    count: usize,
    emitted: bool,
    providers: BTreeSet<String>,
    users: BTreeSet<String>,
    first_ts: Option<u64>,
}

impl EmptyResponseTracker {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            count: 0,
            emitted: false,
            providers: BTreeSet::new(),
            users: BTreeSet::new(),
            first_ts: None,
        }
    }

    /// Record one receipt. Returns the resulting consecutive count.
    /// Emits `EmptyResponseStreak` directly via the sink exactly once when
    /// the count first hits [`STREAK_THRESHOLD`]. Used by code paths that
    /// have a live `EventSink` in scope (e.g. hero-judge completion loop).
    pub fn record(&mut self, receipt: &ModelCallReceipt, sink: &EventSink) -> Result<usize> {
        let (count, payload) = self.observe(receipt);
        if let Some(payload) = payload {
            sink.emit(EventKind::EmptyResponseStreak, payload)?;
        }
        Ok(count)
    }

    /// Record one receipt into a queued-events vector (deferred emission).
    /// Used by `complete_structured_model_only` and other paths that
    /// collect events into a `Vec<(EventKind, Value)>` for the caller
    /// to drain after joining concurrent lanes.
    pub fn record_into_queue(
        &mut self,
        receipt: &ModelCallReceipt,
        queue: &mut Vec<(EventKind, Value)>,
    ) -> usize {
        let (count, payload) = self.observe(receipt);
        if let Some(payload) = payload {
            queue.push((EventKind::EmptyResponseStreak, payload));
        }
        count
    }

    /// Shared bookkeeping. Returns `(count, Some(emit_payload))` if and only
    /// if the call advanced the streak from `THRESHOLD-1` to `THRESHOLD`.
    fn observe(&mut self, receipt: &ModelCallReceipt) -> (usize, Option<Value>) {
        let bytes = receipt.response.as_deref().map(str::len).unwrap_or(0);
        if bytes > 0 {
            self.reset();
            return (0, None);
        }
        if self.first_ts.is_none() {
            self.first_ts = Some(crate::events::now_epoch_secs());
        }
        self.count += 1;
        self.providers.insert(receipt.provider.clone());
        if let Some(user) = receipt
            .credential_user_id
            .as_deref()
            .or(receipt.selected_credential_user_id.as_deref())
        {
            self.users.insert(user.to_string());
        }
        let payload = (self.count == STREAK_THRESHOLD && !self.emitted).then(|| {
            self.emitted = true;
            json!({
                "kind": self.kind,
                "count": self.count,
                "providers_tried": self.providers.iter().collect::<Vec<_>>(),
                "users_tried": self.users.iter().collect::<Vec<_>>(),
                "first_attempt_ts": self.first_ts,
                "last_attempt_ts": crate::events::now_epoch_secs(),
            })
        });
        (self.count, payload)
    }

    fn reset(&mut self) {
        self.count = 0;
        self.emitted = false;
        self.providers.clear();
        self.users.clear();
        self.first_ts = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn empty_receipt(provider: &str, user: &str) -> ModelCallReceipt {
        ModelCallReceipt {
            id: "test".into(),
            kind: "stage_brainstorm".into(),
            task_id: None,
            provider: provider.into(),
            model: "jnoccio-router".into(),
            latency_ms: 100,
            success: true,
            cost_usd: None,
            response: None,
            error: None,
            budget_used: None,
            budget_remaining: None,
            route: None,
            credential_policy: None,
            selected_credential_user_id: Some(user.into()),
            credential_user_id: Some(user.into()),
            retry_count: Some(0),
            quality_band: None,
        }
    }

    fn nonempty_receipt() -> ModelCallReceipt {
        let mut r = empty_receipt("jnoccio", "user_1");
        r.response = Some("{\"k\":1}".to_string());
        r
    }

    const SERIALIZED_KIND: &str = "empty_response_streak"; // EventKind is rename_all="snake_case".

    fn read_log(dir: &std::path::Path) -> String {
        std::fs::read_to_string(dir.join("target/zyal/runner-events.jsonl")).unwrap_or_default()
    }

    #[test]
    fn single_empty_does_not_emit() {
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-1").unwrap();
        let mut tracker = EmptyResponseTracker::new("stage_brainstorm");
        let n = tracker
            .record(&empty_receipt("jnoccio", "user_1"), &sink)
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(read_log(dir.path()).matches(SERIALIZED_KIND).count(), 0);
    }

    #[test]
    fn three_empties_emit_once() {
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-2").unwrap();
        let mut tracker = EmptyResponseTracker::new("stage_brainstorm");
        tracker
            .record(&empty_receipt("jnoccio", "user_1"), &sink)
            .unwrap();
        tracker
            .record(&empty_receipt("jnoccio", "user_2"), &sink)
            .unwrap();
        let n = tracker
            .record(&empty_receipt("jnoccio", "user_1"), &sink)
            .unwrap();
        assert_eq!(n, 3);
        assert_eq!(
            read_log(dir.path()).matches(SERIALIZED_KIND).count(),
            1,
            "streak event must emit exactly once"
        );
        // 4th empty should not double-emit.
        tracker
            .record(&empty_receipt("jnoccio", "user_2"), &sink)
            .unwrap();
        assert_eq!(read_log(dir.path()).matches(SERIALIZED_KIND).count(), 1);
    }

    #[test]
    fn nonempty_resets_streak() {
        let dir = tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-3").unwrap();
        let mut tracker = EmptyResponseTracker::new("stage_brainstorm");
        tracker
            .record(&empty_receipt("jnoccio", "user_1"), &sink)
            .unwrap();
        tracker
            .record(&empty_receipt("jnoccio", "user_2"), &sink)
            .unwrap();
        // A non-empty receipt clears the streak.
        let n = tracker.record(&nonempty_receipt(), &sink).unwrap();
        assert_eq!(n, 0);
        // Now two more empties — still below threshold.
        tracker
            .record(&empty_receipt("jnoccio", "user_1"), &sink)
            .unwrap();
        let n = tracker
            .record(&empty_receipt("jnoccio", "user_2"), &sink)
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(read_log(dir.path()).matches(SERIALIZED_KIND).count(), 0);
    }
}
