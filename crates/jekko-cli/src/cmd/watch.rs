//! `jekko watch <run_id>` — tail a ZYAL run's NDJSON event stream and emit
//! per-tick snapshots + remediation actions.
//!
//! Wires Phase G1's pure `fold_events` + `detect_and_remediate` (in the
//! `jankurai-runner` crate) to a shippable operator surface. The watcher
//! opens `target/zyal/runs/<run_id>/events.jsonl`, reads any existing lines,
//! and then either exits (`--once`) or follows the file via the `notify`
//! crate, re-folding on every append batch.
//!
//! Three output formats:
//! - `plain` (default) — newline-delimited per-event summary plus any
//!   triggered remediation rules.
//! - `json` — pretty-printed `{snapshot, actions}` JSON object per tick.
//! - `tui` — Phase G2's Ratatui dashboard: a single-screen live view with
//!   three top panes (Lanes / Parity / Model), an active remediation rules
//!   list, and a Jankurai status row. Refreshes every 1s (or on file event
//!   from `notify`). Quits on `q`, `Esc`, or `Ctrl-C`. For CI-friendly
//!   testing, `--tui-once-snapshot` renders a single frame to a
//!   [`ratatui::backend::TestBackend`] and dumps the buffer to stdout.
//!
//! All four remediation rules surface: `stall_detected`,
//! `provider_error_burst`, `parity_gaps_growing`, `jankurai_regression`.

mod json;
mod tail;
mod tui;

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use jankurai_runner::events::{run_event_file_rel, Event};
use jankurai_runner::watcher::{
    detect_and_remediate, fold_events, RemediationAction, RemediationRule, WatcherSnapshot,
};

use crate::cli::GlobalOpts;
use json::emit_json;
use tail::{follow, read_from_offset};
use tui::run_tui;

/// `jekko watch <run_id>` arguments.
#[derive(Args, Debug)]
pub struct WatchArgs {
    /// Run id to watch. Looks up `target/zyal/runs/<run_id>/events.jsonl`
    /// relative to the current working directory (or the explicit
    /// `--repo-root`).
    #[arg(value_name = "RUN_ID")]
    pub run_id: String,

    /// Override the repo root used to locate the events file. Defaults to
    /// the current working directory.
    #[arg(long, value_name = "PATH")]
    pub repo_root: Option<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = WatchFormat::Plain)]
    pub format: WatchFormat,

    /// Read the existing file once and exit without watching for new events.
    #[arg(long)]
    pub once: bool,

    /// Disable the default file-follow behavior. Equivalent to `--once`
    /// after the initial drain.
    #[arg(long = "no-follow")]
    pub no_follow: bool,

    /// Seconds without a progress event before `StallDetected` fires.
    #[arg(long = "stall-threshold", default_value_t = 300, value_name = "SECS")]
    pub stall_threshold: u64,

    /// Provider error rate (0.0..=1.0) that triggers
    /// `ProviderErrorBurst`.
    #[arg(
        long = "error-rate-threshold",
        default_value_t = 0.5,
        value_name = "FLOAT"
    )]
    pub error_rate_threshold: f64,

    /// (Phase G2 / `--format tui` only) Render exactly one frame into a
    /// `ratatui::backend::TestBackend`, dump the rendered buffer to stdout
    /// as plain text, then exit `0`. Lets CI assert on the dashboard layout
    /// without a real terminal.
    #[arg(long = "tui-once-snapshot")]
    pub tui_once_snapshot: bool,
}

/// Output format for the watcher.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum WatchFormat {
    /// Newline-delimited human-readable summary.
    Plain,
    /// Pretty JSON `{snapshot, actions}` per tick.
    Json,
    /// Phase G2 Ratatui dashboard. Interactive by default; pass
    /// `--tui-once-snapshot` to render a single frame to a
    /// `TestBackend` and dump it for CI.
    Tui,
}

/// Default debounce window between file-change events. Notify fires once per
/// platform-specific atomic write batch; we coalesce within this window so a
/// single 5-line append doesn't yield 5 separate ticks.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Entry point invoked from `main.rs`.
pub fn run(_global: &GlobalOpts, args: &WatchArgs) -> Result<()> {
    let repo_root = match args.repo_root.clone() {
        Some(p) => p,
        None => std::env::current_dir().context("resolving working directory")?,
    };
    let events_path = repo_root.join(run_event_file_rel(&args.run_id));

    // TUI mode owns its own event loop (terminal setup, key handling,
    // refresh cadence). It also has a CI-friendly one-shot variant.
    if args.format == WatchFormat::Tui {
        return run_tui(&events_path, args);
    }

    // Drain whatever already exists. The file may not be present yet — in
    // that case `read_from_offset` returns an empty initial tick.
    let mut offset: u64 = 0;
    let mut tick_state = TickState::default();
    let (new_events, new_offset) = read_from_offset(&events_path, offset)?;
    offset = new_offset;
    emit_tick(
        &new_events,
        &mut tick_state,
        args,
        args.format,
        /* initial */ true,
    )?;

    if args.once || args.no_follow {
        return Ok(());
    }

    follow(&events_path, offset, &mut tick_state, args, args.format)
}

/// Per-tick mutable state carried across iterations of the watch loop.
#[derive(Default)]
struct TickState {
    /// Rolling buffer of every event observed so far. We re-fold from
    /// scratch on every tick so the snapshot stays consistent even if a
    /// previous append failed mid-line.
    all_events: Vec<Event>,
    /// Most recent `parity_gaps_open` value, three ticks ago. We approximate
    /// the spec's "growing for 3 ticks" by comparing the current value
    /// against the value from up to three observations back.
    prior_gaps_history: Vec<i64>,
    /// Last `jankurai_hard_findings` value we observed; used to detect
    /// regression. None until the first audit event.
    prior_hard_findings: Option<i64>,
}

/// Compute the snapshot, run remediation, and emit per-format output for a
/// single batch of new events.
fn emit_tick(
    new_events: &[Event],
    state: &mut TickState,
    args: &WatchArgs,
    format: WatchFormat,
    initial: bool,
) -> Result<()> {
    state.all_events.extend(new_events.iter().cloned());
    let snap = fold_events(&state.all_events);

    let prior_gaps = if state.prior_gaps_history.len() >= 3 {
        Some(state.prior_gaps_history[state.prior_gaps_history.len() - 3])
    } else {
        None
    };

    let now_ts = now_epoch_secs();
    let actions = detect_and_remediate(
        &snap,
        now_ts,
        args.stall_threshold,
        args.error_rate_threshold,
        prior_gaps,
        state.prior_hard_findings,
    );

    match format {
        WatchFormat::Plain => emit_plain(new_events, &snap, &actions, initial)?,
        WatchFormat::Json => emit_json(&snap, &actions)?,
        WatchFormat::Tui => unreachable!("tui handled in run_tui"),
    }

    state.prior_gaps_history.push(snap.parity_gaps_open);
    if let Some(hf) = snap.last_jankurai_hard_findings {
        state.prior_hard_findings = Some(hf);
    }
    Ok(())
}

fn emit_plain(
    new_events: &[Event],
    snap: &WatcherSnapshot,
    actions: &[RemediationAction],
    initial: bool,
) -> Result<()> {
    if initial {
        println!(
            "watcher: starting (events_seen={}, lanes_started={}, lanes_finished={})",
            snap.lanes_started + snap.lanes_finished,
            snap.lanes_started,
            snap.lanes_finished
        );
    }
    for ev in new_events {
        let kind = serde_json::to_string(&ev.kind)
            .unwrap_or_else(|_| "\"unknown\"".to_string())
            .trim_matches('"')
            .to_string();
        println!("ts={} kind={} run={}", ev.ts, kind, ev.run_id);
    }
    println!(
        "snapshot: lanes={}/{} workers_pass={} workers_fail={} gaps_open={} model_attempts={} model_failures={} spend_usd={:.4}",
        snap.lanes_started,
        snap.lanes_finished,
        snap.workers_pass,
        snap.workers_fail,
        snap.parity_gaps_open,
        snap.model_attempts,
        snap.model_failures,
        snap.model_spend_usd
    );
    for action in actions {
        println!(
            "remediation: rule={} summary={}",
            rule_label(action.rule),
            action.summary
        );
        for (k, v) in &action.detail {
            println!("  {k}={v}");
        }
    }
    Ok(())
}

/// Stable string label for a [`RemediationRule`] — matches the spec names
/// used in the per-tick output and in tests.
fn rule_label(rule: RemediationRule) -> &'static str {
    match rule {
        RemediationRule::StallDetected => "stall_detected",
        RemediationRule::ProviderErrorBurst => "provider_error_burst",
        RemediationRule::ParityGapsGrowing => "parity_gaps_growing",
        RemediationRule::JankuraiRegression => "jankurai_regression",
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ─── Phase G2: Ratatui dashboard ─────────────────────────────────────────
//
// Moved to `tui.rs`. See `tui::run_tui` for the entry point.
