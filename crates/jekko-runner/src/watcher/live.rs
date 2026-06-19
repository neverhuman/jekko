//! Live-frame builders: convert the pure [`WatcherSnapshot`] into the compact
//! NDJSON events that drive the jekko-web diagram's live boxes (global KPIs,
//! watcher equations/values, per-agent status, data-feed ticks, rendered
//! artifacts). Payloads are deliberately small — IDs + scalar values only —
//! so each line stays under [`crate::events::MAX_LINE_BYTES`]; the SPA holds
//! the full history and backfills omitted fields from its schema defaults.

use serde_json::{json, Value};

use super::metrics::WatcherSnapshot;
use crate::events::{EventKind, EventSink};

/// Round to 4 decimal places to keep float payloads short and bounded.
fn round4(value: f64) -> f64 {
    if value.is_finite() {
        (value * 10_000.0).round() / 10_000.0
    } else {
        0.0
    }
}

fn status_for_error_rate(rate: f64) -> &'static str {
    if rate >= 0.5 {
        "alert"
    } else if rate >= 0.2 {
        "warn"
    } else {
        "green"
    }
}

/// `kpi_update` payload — matches the web `Kpi` contract (omitted fields backfill).
pub fn kpi_update(id: &str, name: &str, value: f64, unit: Option<&str>, status: &str) -> Value {
    json!({ "kpi": { "id": id, "name": name, "value": round4(value), "unit": unit, "status": status } })
}

/// `watcher_update` payload — matches the web `Watcher` contract.
pub fn watcher_update(id: &str, kind: &str, label: &str, expr: &str, value: f64) -> Value {
    json!({ "watcher": { "id": id, "kind": kind, "label": label, "expr": expr, "current_value": round4(value) } })
}

/// `agent_status` payload — one flattened agent-flow box's live status.
pub fn agent_status(
    node_id: &str,
    status: &str,
    progress: Option<f64>,
    tokens: Option<u64>,
) -> Value {
    json!({ "node_id": node_id, "status": status, "progress": progress.map(round4), "tokens": tokens })
}

/// `feed_tick` payload — a live data-feed source box.
pub fn feed_tick(id: &str, kind: &str, label: &str, latest_value: f64) -> Value {
    json!({ "feed": { "id": id, "kind": kind, "label": label, "latest_value": round4(latest_value) } })
}

/// `artifact_rendered` payload — a freshly rendered artifact preview.
pub fn artifact_rendered(artifact_id: &str, content_url: &str, mime_type: &str) -> Value {
    json!({ "artifact_id": artifact_id, "content_url": content_url, "mime_type": mime_type })
}

/// The KPI + watcher frames derived from a snapshot, as `(kind, data)` tuples.
pub fn snapshot_frames(
    snap: &WatcherSnapshot,
    first_ts: Option<u64>,
    now_ts: u64,
) -> Vec<(EventKind, Value)> {
    let error_rate = snap.error_rate();
    let mut frames = vec![
        (
            EventKind::KpiUpdate,
            kpi_update(
                "cost",
                "Live cost",
                snap.model_spend_usd,
                Some("USD"),
                "green",
            ),
        ),
        (
            EventKind::KpiUpdate,
            kpi_update(
                "error_rate",
                "Error rate",
                error_rate * 100.0,
                Some("%"),
                status_for_error_rate(error_rate),
            ),
        ),
        (
            EventKind::KpiUpdate,
            kpi_update(
                "throughput",
                "Throughput",
                snap.lanes_per_minute(first_ts, now_ts),
                Some("/min"),
                "green",
            ),
        ),
        (
            EventKind::KpiUpdate,
            kpi_update(
                "parity_gaps",
                "Open parity gaps",
                snap.parity_gaps_open as f64,
                None,
                if snap.parity_gaps_open > 0 {
                    "warn"
                } else {
                    "green"
                },
            ),
        ),
        (
            EventKind::WatcherUpdate,
            watcher_update(
                "provider_error",
                "equation",
                "Provider error",
                "failures / attempts",
                error_rate,
            ),
        ),
    ];
    if let Some(score) = snap.last_jankurai_score {
        frames.push((
            EventKind::KpiUpdate,
            kpi_update("jankurai", "Jankurai score", score as f64, None, "green"),
        ));
    }
    frames
}

/// Emit a fresh batch of KPI + watcher frames for the current snapshot.
pub fn emit_live_snapshot(
    sink: &EventSink,
    snap: &WatcherSnapshot,
    first_ts: Option<u64>,
    now_ts: u64,
) -> anyhow::Result<()> {
    for (kind, data) in snapshot_frames(snap, first_ts, now_ts) {
        sink.emit(kind, data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Event, MAX_LINE_BYTES};

    fn line_len(kind: EventKind, data: &Value) -> usize {
        let event = Event {
            ts: 1_780_000_000,
            kind,
            run_id: "run-superreasoning-openqg-01".into(),
            event_offset: 0,
            data: data.clone(),
        };
        serde_json::to_string(&event).unwrap().len()
    }

    #[test]
    fn all_frames_stay_under_the_line_cap() {
        let snap = WatcherSnapshot {
            lanes_finished: 42,
            model_attempts: 200,
            model_failures: 30,
            parity_gaps_open: 7,
            model_spend_usd: 18.4231,
            last_jankurai_score: Some(91),
            ..Default::default()
        };
        for (kind, data) in snapshot_frames(&snap, Some(0), 600) {
            assert!(
                line_len(kind, &data) <= MAX_LINE_BYTES,
                "frame too big: {data}"
            );
        }
        for data in [
            agent_status("route_fusion/draft-3", "running", Some(0.5), Some(12345)),
            feed_tick("market_feed", "stock_ticker", "Market feed", 5431.2),
            artifact_rendered(
                "report_pdf",
                "/api/v1/artifacts/report_pdf/p1.png",
                "image/png",
            ),
        ] {
            assert!(line_len(EventKind::AgentStatus, &data) <= MAX_LINE_BYTES);
        }
    }

    #[test]
    fn snapshot_frames_carry_cost_error_and_jankurai() {
        let snap = WatcherSnapshot {
            model_attempts: 10,
            model_failures: 5,
            model_spend_usd: 2.5,
            last_jankurai_score: Some(88),
            ..Default::default()
        };
        let frames = snapshot_frames(&snap, Some(0), 60);
        let cost = frames
            .iter()
            .find(|(_, d)| d["kpi"]["id"] == "cost")
            .unwrap();
        assert_eq!(cost.1["kpi"]["value"], 2.5);
        let err = frames
            .iter()
            .find(|(_, d)| d["kpi"]["id"] == "error_rate")
            .unwrap();
        assert_eq!(err.1["kpi"]["value"], 50.0);
        assert_eq!(err.1["kpi"]["status"], "alert");
        assert!(frames.iter().any(|(_, d)| d["kpi"]["id"] == "jankurai"));
        // watcher frame present with an equation
        assert!(frames.iter().any(|(k, d)| *k == EventKind::WatcherUpdate
            && d["watcher"]["expr"] == "failures / attempts"));
    }

    #[test]
    fn no_jankurai_frame_without_a_score() {
        let frames = snapshot_frames(&WatcherSnapshot::default(), None, 0);
        assert!(!frames.iter().any(|(_, d)| d["kpi"]["id"] == "jankurai"));
    }

    #[test]
    fn frames_round_trip_through_the_event_sink() {
        let dir = tempfile::tempdir().unwrap();
        let sink = EventSink::open(dir.path(), "run-1").unwrap();
        emit_live_snapshot(
            &sink,
            &WatcherSnapshot {
                model_spend_usd: 1.0,
                ..Default::default()
            },
            Some(0),
            60,
        )
        .unwrap();
        let text = std::fs::read_to_string(sink.path()).unwrap();
        assert!(text.contains("kpi_update"));
        assert!(text.contains("watcher_update"));
        // every emitted line parses back into an Event
        for line in text.lines() {
            let _event: Event = serde_json::from_str(line).unwrap();
        }
    }
}
