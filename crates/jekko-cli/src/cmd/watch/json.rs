use std::collections::BTreeMap;

use anyhow::Result;
use jankurai_runner::watcher::{RemediationAction, WatcherSnapshot};
use serde::Serialize;

use super::rule_label;

pub(super) fn emit_json(snap: &WatcherSnapshot, actions: &[RemediationAction]) -> Result<()> {
    let payload = JsonTick {
        snapshot: SnapshotJson::from(snap),
        actions: actions.iter().map(ActionJson::from).collect(),
    };
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

#[derive(Serialize)]
struct JsonTick {
    snapshot: SnapshotJson,
    actions: Vec<ActionJson>,
}

#[derive(Serialize)]
struct SnapshotJson {
    lanes_started: u64,
    lanes_finished: u64,
    workers_pass: u64,
    workers_fail: u64,
    parity_gaps_open: i64,
    parity_gaps_closed: u64,
    model_attempts: u64,
    model_failures: u64,
    errors_by_provider: BTreeMap<String, u64>,
    model_spend_usd: f64,
    last_progress_ts: Option<u64>,
    last_jankurai_score: Option<i64>,
    last_jankurai_hard_findings: Option<i64>,
    finished: bool,
    error_rate: f64,
}

impl From<&WatcherSnapshot> for SnapshotJson {
    fn from(snap: &WatcherSnapshot) -> Self {
        Self {
            lanes_started: snap.lanes_started,
            lanes_finished: snap.lanes_finished,
            workers_pass: snap.workers_pass,
            workers_fail: snap.workers_fail,
            parity_gaps_open: snap.parity_gaps_open,
            parity_gaps_closed: snap.parity_gaps_closed,
            model_attempts: snap.model_attempts,
            model_failures: snap.model_failures,
            errors_by_provider: snap.errors_by_provider.clone(),
            model_spend_usd: snap.model_spend_usd,
            last_progress_ts: snap.last_progress_ts,
            last_jankurai_score: snap.last_jankurai_score,
            last_jankurai_hard_findings: snap.last_jankurai_hard_findings,
            finished: snap.finished,
            error_rate: snap.error_rate(),
        }
    }
}

#[derive(Serialize)]
struct ActionJson {
    rule: &'static str,
    summary: String,
    detail: BTreeMap<String, String>,
}

impl From<&RemediationAction> for ActionJson {
    fn from(action: &RemediationAction) -> Self {
        Self {
            rule: rule_label(action.rule),
            summary: action.summary.clone(),
            detail: action.detail.clone(),
        }
    }
}
