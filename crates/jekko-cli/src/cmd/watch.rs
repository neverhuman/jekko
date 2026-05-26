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

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use crossterm::event::{self as ct_event, Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use jankurai_runner::events::{run_event_file_rel, Event};
use jankurai_runner::watcher::{
    detect_and_remediate, fold_events, RemediationAction, RemediationRule, WatcherSnapshot,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use serde::Serialize;

use crate::cli::GlobalOpts;

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

fn follow(
    path: &Path,
    mut offset: u64,
    state: &mut TickState,
    args: &WatchArgs,
    format: WatchFormat,
) -> Result<()> {
    // Watch the *parent* directory so we still receive events if the file
    // doesn't exist yet (notify can't subscribe to a missing path).
    let watch_dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    if !watch_dir.exists() {
        std::fs::create_dir_all(&watch_dir)
            .with_context(|| format!("mkdir -p {}", watch_dir.display()))?;
    }

    let (tx, rx) = channel::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .context("create file watcher")?;
    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("watch {}", watch_dir.display()))?;

    loop {
        // Wait for a change. We also wake every 5s so stall detection can
        // fire even when there's no fs activity.
        let timeout = Duration::from_secs(5);
        match rx.recv_timeout(timeout) {
            Ok(Ok(_event)) => {
                // Coalesce burst of notifications within the debounce window
                // before reading the file.
                let deadline = std::time::Instant::now() + DEBOUNCE;
                while let Ok(remaining) = deadline
                    .checked_duration_since(std::time::Instant::now())
                    .ok_or(())
                {
                    match rx.recv_timeout(remaining) {
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
            }
            Ok(Err(err)) => {
                eprintln!("watch error: {err}");
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {
                // Periodic tick so stall rules still fire on a quiet stream.
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let (new_events, new_offset) = read_from_offset(path, offset)?;
        offset = new_offset;
        emit_tick(&new_events, state, args, format, /* initial */ false)?;

        if state
            .all_events
            .iter()
            .any(|ev| matches!(ev.kind, jankurai_runner::events::EventKind::RunFinished))
        {
            break;
        }
    }
    Ok(())
}

/// Read any new lines appended past `offset` and return the parsed events
/// plus the new offset. Lines that fail to parse are skipped (with a stderr
/// notice) so a single malformed event doesn't kill the watcher.
fn read_from_offset(path: &Path, offset: u64) -> Result<(Vec<Event>, u64)> {
    if !path.exists() {
        return Ok((Vec::new(), 0));
    }
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let len = file.metadata()?.len();
    // File was rotated / truncated — restart from the beginning.
    let read_from = if offset > len { 0 } else { offset };
    file.seek(SeekFrom::Start(read_from))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut consumed = read_from;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                eprintln!("read {}: {err}", path.display());
                break;
            }
        };
        // +1 for the newline that BufRead stripped.
        consumed = consumed.saturating_add(line.len() as u64 + 1);
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(ev) => events.push(ev),
            Err(err) => {
                eprintln!("skip malformed event line: {err}");
            }
        }
    }
    // Clamp consumed to the real file length so we don't drift past EOF if
    // the last line had no trailing newline.
    let consumed = consumed.min(len);
    Ok((events, consumed))
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

fn emit_json(snap: &WatcherSnapshot, actions: &[RemediationAction]) -> Result<()> {
    let payload = JsonTick {
        snapshot: SnapshotJson::from(snap),
        actions: actions.iter().map(ActionJson::from).collect(),
    };
    println!("{}", serde_json::to_string_pretty(&payload)?);
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

// ─── Phase G2: Ratatui dashboard ─────────────────────────────────────────

/// All state the TUI needs to render a single frame.
#[derive(Clone, Debug)]
struct DashboardState {
    run_id: String,
    start_ts: Option<u64>,
    now_ts: u64,
    snap: WatcherSnapshot,
    actions: Vec<RemediationAction>,
    /// Visual offset into the active-rules list for j/k scrolling.
    rules_scroll: usize,
}

impl DashboardState {
    fn elapsed_label(&self) -> String {
        let start = self.start_ts.unwrap_or(self.now_ts);
        let secs = self.now_ts.saturating_sub(start);
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{h}:{m:02}:{s:02}")
    }
}

/// Refresh interval when no `notify` event arrives. 1s matches the spec
/// header. The crossterm poll loop wakes once per tick to give the user a
/// chance to press q/Esc/Ctrl-C.
const TUI_TICK: Duration = Duration::from_millis(1000);
/// Sub-tick used inside the keyboard-poll loop so q/Esc/Ctrl-C feel snappy
/// even when nothing else is happening.
const TUI_POLL: Duration = Duration::from_millis(100);

/// Entry point for `--format tui`. Handles the one-shot snapshot path and
/// the live event loop.
fn run_tui(events_path: &Path, args: &WatchArgs) -> Result<()> {
    // Always drain whatever is already on disk so the very first frame is
    // representative.
    let mut offset: u64 = 0;
    let mut tick_state = TickState::default();
    let (initial_events, new_offset) = read_from_offset(events_path, offset)?;
    offset = new_offset;

    let mut dash = build_dashboard(&initial_events, &mut tick_state, args);

    if args.tui_once_snapshot {
        return render_once_snapshot(&dash);
    }

    if args.once || args.no_follow {
        // Render one frame to the real terminal, then exit. No event loop,
        // no notify watcher.
        let mut terminal = enter_terminal()?;
        let render_err = terminal
            .draw(|f| draw_dashboard(f, &dash))
            .err()
            .map(|e| anyhow::anyhow!(e));
        let _ = leave_terminal(&mut terminal);
        if let Some(err) = render_err {
            return Err(err.context("render initial tui frame"));
        }
        return Ok(());
    }

    // Live mode: set up notify on the parent dir, raw terminal, and a poll
    // loop that wakes on key events, file events, or the 1s tick.
    let watch_dir = events_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    if !watch_dir.exists() {
        std::fs::create_dir_all(&watch_dir)
            .with_context(|| format!("mkdir -p {}", watch_dir.display()))?;
    }
    let (tx, rx) = channel::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .context("create file watcher")?;
    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("watch {}", watch_dir.display()))?;

    let mut terminal = enter_terminal()?;
    let loop_result = (|| -> Result<()> {
        let mut last_draw = std::time::Instant::now();
        terminal.draw(|f| draw_dashboard(f, &dash))?;
        loop {
            // Keyboard poll — short timeout so quits feel responsive.
            if ct_event::poll(TUI_POLL).unwrap_or(false) {
                match ct_event::read() {
                    Ok(CtEvent::Key(key)) if key.kind != KeyEventKind::Release => {
                        if quit_requested(key.code, key.modifiers) {
                            break;
                        }
                        match key.code {
                            KeyCode::Char('j') => {
                                dash.rules_scroll = dash.rules_scroll.saturating_add(1);
                            }
                            KeyCode::Char('k') => {
                                dash.rules_scroll = dash.rules_scroll.saturating_sub(1);
                            }
                            _ => {}
                        }
                        terminal.draw(|f| draw_dashboard(f, &dash))?;
                    }
                    Ok(_) => {
                        terminal.draw(|f| draw_dashboard(f, &dash))?;
                    }
                    Err(_) => break,
                }
            }

            // Drain any file events without blocking. If anything fired we
            // re-read the events file and rebuild the dashboard state.
            let mut had_fs = false;
            while let Ok(_msg) = rx.try_recv() {
                had_fs = true;
            }
            if had_fs {
                let (new_events, new_offset) = read_from_offset(events_path, offset)?;
                offset = new_offset;
                dash = build_dashboard_continue(&new_events, &mut tick_state, args, &dash);
            }

            // Periodic redraw + stall-rule refresh once per tick.
            if last_draw.elapsed() >= TUI_TICK {
                dash = build_dashboard_continue(&[], &mut tick_state, args, &dash);
                terminal.draw(|f| draw_dashboard(f, &dash))?;
                last_draw = std::time::Instant::now();
            }

            if dash.snap.finished {
                // One last refresh to make sure the operator sees the final
                // numbers, then exit.
                terminal.draw(|f| draw_dashboard(f, &dash))?;
                break;
            }

            // Disconnected sender means the watcher thread died.
            if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(Duration::from_millis(0)) {
                break;
            }
        }
        Ok(())
    })();
    leave_terminal(&mut terminal)?;
    loop_result
}

/// Build a fresh `DashboardState` from a brand-new event batch and the
/// initial (empty) `TickState`.
fn build_dashboard(
    new_events: &[Event],
    tick_state: &mut TickState,
    args: &WatchArgs,
) -> DashboardState {
    tick_state.all_events.extend(new_events.iter().cloned());
    let snap = fold_events(&tick_state.all_events);
    let now_ts = now_epoch_secs();
    let prior_gaps = if tick_state.prior_gaps_history.len() >= 3 {
        Some(tick_state.prior_gaps_history[tick_state.prior_gaps_history.len() - 3])
    } else {
        None
    };
    let actions = detect_and_remediate(
        &snap,
        now_ts,
        args.stall_threshold,
        args.error_rate_threshold,
        prior_gaps,
        tick_state.prior_hard_findings,
    );
    tick_state.prior_gaps_history.push(snap.parity_gaps_open);
    if let Some(hf) = snap.last_jankurai_hard_findings {
        tick_state.prior_hard_findings = Some(hf);
    }
    let start_ts = tick_state.all_events.first().map(|e| e.ts);
    DashboardState {
        run_id: args.run_id.clone(),
        start_ts,
        now_ts,
        snap,
        actions,
        rules_scroll: 0,
    }
}

/// Continue an existing dashboard: fold in any new events and re-run the
/// rules, preserving scroll position.
fn build_dashboard_continue(
    new_events: &[Event],
    tick_state: &mut TickState,
    args: &WatchArgs,
    prev: &DashboardState,
) -> DashboardState {
    let mut next = build_dashboard(new_events, tick_state, args);
    next.rules_scroll = prev.rules_scroll;
    next
}

/// Render one frame into a `TestBackend`, dump the rendered text to stdout,
/// and return. The output is plain (no ANSI), so tests can grep it.
fn render_once_snapshot(dash: &DashboardState) -> Result<()> {
    const W: u16 = 120;
    const H: u16 = 30;
    let backend = TestBackend::new(W, H);
    let mut terminal = Terminal::new(backend).context("init test backend")?;
    terminal
        .draw(|f| draw_dashboard(f, dash))
        .context("draw to test backend")?;
    // `Buffer::to_string` would lose line breaks; iterate row-by-row instead.
    let buf = terminal.backend().buffer().clone();
    let mut out = String::with_capacity((W as usize + 1) * H as usize);
    for y in 0..H {
        for x in 0..W {
            let cell = &buf[(x, y)];
            out.push_str(cell.symbol());
        }
        out.push('\n');
    }
    println!("{out}");
    Ok(())
}

/// Translate a keystroke into a quit decision. `q`, `Esc`, and `Ctrl-C`
/// all quit; anything else falls through.
fn quit_requested(code: KeyCode, mods: KeyModifiers) -> bool {
    matches!(code, KeyCode::Esc | KeyCode::Char('q'))
        || (code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL))
}

/// RAII-style terminal init: raw mode + alt screen.
fn enter_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("init crossterm terminal")
}

/// Tear down the terminal regardless of why we're exiting.
fn leave_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    Ok(())
}

/// Render the dashboard widgets into the frame. Pure with respect to
/// `dash` — callable from both the real terminal and `TestBackend`.
fn draw_dashboard(f: &mut Frame<'_>, dash: &DashboardState) {
    let area = f.area();
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![
            Span::raw(" ZYAL Watcher: "),
            Span::styled(
                dash.run_id.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("   elapsed "),
            Span::styled(
                dash.elapsed_label(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Vertical layout: top stats row, rules list, jankurai row, hint.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // top stat panes
            Constraint::Min(3),    // active rules
            Constraint::Length(3), // jankurai row
            Constraint::Length(1), // key hint
        ])
        .split(inner);

    draw_stat_row(f, chunks[0], dash);
    draw_rules(f, chunks[1], dash);
    draw_jankurai(f, chunks[2], dash);
    draw_hint(f, chunks[3]);
}

fn draw_stat_row(f: &mut Frame<'_>, area: Rect, dash: &DashboardState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    let lanes = vec![
        Line::from(format!("started:      {}", dash.snap.lanes_started)),
        Line::from(format!("finished:     {}", dash.snap.lanes_finished)),
        Line::from(format!("workers ok:   {}", dash.snap.workers_pass)),
        Line::from(format!("workers fail: {}", dash.snap.workers_fail)),
    ];
    let lanes_widget =
        Paragraph::new(lanes).block(Block::default().borders(Borders::ALL).title(" Lanes "));
    f.render_widget(lanes_widget, cols[0]);

    let parity = vec![
        Line::from(format!("open:    {}", dash.snap.parity_gaps_open)),
        Line::from(format!("closed:  {}", dash.snap.parity_gaps_closed)),
    ];
    let parity_widget =
        Paragraph::new(parity).block(Block::default().borders(Borders::ALL).title(" Parity "));
    f.render_widget(parity_widget, cols[1]);

    let err_pct = dash.snap.error_rate() * 100.0;
    let model = vec![
        Line::from(format!("attempts:    {}", dash.snap.model_attempts)),
        Line::from(format!("failures:    {}", dash.snap.model_failures)),
        Line::from(format!("error rate:  {err_pct:.1}%")),
        Line::from(format!("spend (usd): ${:.2}", dash.snap.model_spend_usd)),
    ];
    let model_widget =
        Paragraph::new(model).block(Block::default().borders(Borders::ALL).title(" Model "));
    f.render_widget(model_widget, cols[2]);
}

fn draw_rules(f: &mut Frame<'_>, area: Rect, dash: &DashboardState) {
    let items: Vec<ListItem> = if dash.actions.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "none firing",
            Style::default().add_modifier(Modifier::DIM),
        )))]
    } else {
        dash.actions
            .iter()
            .skip(dash.rules_scroll)
            .map(|a| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        rule_label(a.rule),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::raw(a.summary.clone()),
                ]))
            })
            .collect()
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Active rules "),
    );
    f.render_widget(list, area);
}

fn draw_jankurai(f: &mut Frame<'_>, area: Rect, dash: &DashboardState) {
    let score = dash
        .snap
        .last_jankurai_score
        .map(|s| s.to_string())
        .unwrap_or_else(|| "-".into());
    let hard = dash
        .snap
        .last_jankurai_hard_findings
        .map(|h| h.to_string())
        .unwrap_or_else(|| "-".into());
    let line = Line::from(format!("score: {score}        hard_findings: {hard}"));
    let widget = Paragraph::new(line)
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title(" Jankurai "));
    f.render_widget(widget, area);
}

fn draw_hint(f: &mut Frame<'_>, area: Rect) {
    let widget = Paragraph::new(Line::from(Span::styled(
        " q quit  |  j/k scroll rules ",
        Style::default().add_modifier(Modifier::DIM),
    )));
    f.render_widget(widget, area);
}
