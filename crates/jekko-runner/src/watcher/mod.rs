//! Live-observability watcher for ZYAL runs.
//!
//! Tails the per-run NDJSON event stream at
//! `target/zyal/runs/<run_id>/events.jsonl`, folds events into a pure
//! [`metrics::WatcherSnapshot`], and applies the rules in
//! [`remediation::detect_and_remediate`] to emit follow-up events
//! ([`WorkerStall`], [`WorkerQuarantine`], [`RemediationTriggered`],
//! [`JankuraiRegression`]).
//!
//! G1 (this commit) ships the pure fold + rule engine. G2 adds the Ratatui
//! dashboard surface; G3 adds the `jekko watch` CLI subcommand and the
//! jnoccio-fusion `/metrics` Prometheus endpoint.
//!
//! [`WorkerStall`]: crate::events::EventKind::WorkerStall
//! [`WorkerQuarantine`]: crate::events::EventKind::WorkerQuarantine
//! [`RemediationTriggered`]: crate::events::EventKind::RemediationTriggered
//! [`JankuraiRegression`]: crate::events::EventKind::JankuraiRegression

pub mod control;
pub mod guards;
pub mod live;
pub mod metrics;
pub mod remediation;
pub mod runtime;

pub use control::{make_control_trigger, ControlAction, ControlTrigger};
pub use guards::{in_holdout, proxy_drift_alert, weighted_kpi, GuardMode};
pub use live::{emit_live_snapshot, snapshot_frames};
pub use metrics::{fold_events, WatcherSnapshot};
pub use remediation::{detect_and_remediate, RemediationAction, RemediationRule};
pub use runtime::{BreachSpec, WatcherKind, WatcherRuntime};
