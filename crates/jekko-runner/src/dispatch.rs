//! Run-loop node dispatch (T6) — the entry point that executes ZYAL Power flow
//! nodes through their real kernels and the run's durable event spine.
//!
//! Each node is dispatched through its real kernel — the M9 tool kernel
//! (`run_tool`, behind the full fail-closed `authorize_tool_call` chain), the M8
//! routing topology (decision → provider calls → committed winner), the M12
//! source runtime (deterministic `FixtureSourceProvider` ticks wrapped as
//! provenance/taint-tagged `ValueEnvelope`s), and the M10 tournament kernel
//! (blind cross-provider judging with an injected deterministic verify/refute).
//! Each step's result is recorded and the matching bounded `EventKind` frame is
//! emitted onto the run's `EventSink`.
//!
//! Determinism + safety are inherited from the kernels and the [`RunContext`]
//! [`SourceMode`]: `Replay` bars adapter `invoke()` (outputs come from the
//! durable receipt log), and `Fake`/CI uses canned adapters + fixture feeds, so
//! a dispatch run is byte-reproducible with no network or secrets. The tool path
//! stays capability-gated (a tainted/forbidden/sandboxed-network call is denied,
//! not run).
//!
//! Kernel execution is decoupled from event emission: a node's kernel runs and
//! its outcome is recorded FIRST, then its frames are emitted with per-frame
//! error isolation. So a single oversized/over-budget frame (the `EventSink`
//! hard-caps lines at 512 bytes) is recorded in [`DispatchReport::errors`] and
//! never aborts the rest of the run — one adversarial node id cannot deny the
//! whole dispatch.
//!
//! Integration status: `run_flow_dispatch` is the dispatch entry point — it opens
//! the same per-run `EventSink` the daemon ticks write and drives the real gated
//! kernels. Binding a compiled flowgraph node kind (`node-types.json`
//! `runtime.executor`) to a [`DispatchNode`] is the compiler→runner glue that
//! will let the production port/reasoning ticks feed this layer; those ticks do
//! not call it yet.

use anyhow::Result;
use paper_builder::{PaperBuildReceipt, PaperBuildRequest, GLOBAL_REF};
use serde_json::{json, Value};
use zyal_core::{ActionClass, TaintSet, ValueEnvelope};

use crate::events::{EventKind, EventSink};
use crate::run_store::RunContext;
use crate::source_runtime::{
    envelope_from_tick, feed_tick_frame, DataFeedSpec, FixtureSourceProvider,
};
use crate::tool_kernel::{
    adapter_for, run_tool, RunToolCtx, ToolCallOutcome, ToolDescriptor, ToolPhase,
};
use crate::tournament::{
    run_tournament, Candidate, JudgeInput, TournamentOutcome, VerifierVerdict,
};

mod paper_build;
use paper_build::plan_paper_builder;

#[cfg(test)]
use paper_build::{resolve_paper_output_dir, validate_paper_artifact_contract};

/// One provider option within a route decision (the minimal local shape the
/// runner needs — full candidate detail lives in the route receipt).
#[derive(Debug, Clone)]
pub struct RouteCandidate {
    pub candidate_id: String,
    pub cost_usd: f64,
}

/// A flow node the run-loop executes this tick.
pub enum DispatchNode {
    /// Run a tool through the M9 kernel (authorize → invoke → receipt).
    Tool {
        descriptor: ToolDescriptor,
        node_id: String,
        input: Value,
        incoming_taint: TaintSet,
        action_class: ActionClass,
    },
    /// Poll a data feed for one tick (deterministic fixture provider).
    Feed { spec: DataFeedSpec, seq: u64 },
    /// Emit the route topology: decision → provider calls → committed winner.
    Route {
        route_id: String,
        policy: String,
        candidates: Vec<RouteCandidate>,
    },
    /// Run one tournament generation (blind judging + deterministic verify/refute).
    Tournament {
        tournament_id: String,
        candidates: Vec<Candidate>,
        judges: Vec<JudgeInput>,
    },
    /// Build a paper artifact bundle through the canonical child workflow.
    PaperBuilder {
        node_id: String,
        request: PaperBuildRequest,
        workflow_ref: String,
        source_hash: String,
        interface_hash: String,
    },
}

/// What a dispatch tick produced (kernel outcomes the caller can persist/fold).
#[derive(Debug, Default)]
pub struct DispatchReport {
    pub tools: Vec<ToolCallOutcome>,
    pub feeds: Vec<ValueEnvelope>,
    /// `(route_id, winner_candidate_id)` for each route that committed a winner.
    pub routes: Vec<(String, Option<String>)>,
    pub tournaments: Vec<TournamentOutcome>,
    pub paper_builds: Vec<PaperBuildReceipt>,
    /// Per-frame emission failures (e.g. a frame that exceeded the 512-byte
    /// `EventSink` budget). Recorded, not fatal — the kernel outcome above is
    /// still captured.
    pub errors: Vec<String>,
}

/// Open the run's event sink and dispatch `nodes` — a tick-shaped entry point so
/// the kernels run against the real per-run event spine. `mode` controls
/// live/replay/fake gating end-to-end.
pub fn run_flow_dispatch(
    repo_root: &std::path::Path,
    run_id: &str,
    mode: crate::run_store::SourceMode,
    nodes: &[DispatchNode],
) -> Result<DispatchReport> {
    let sink = EventSink::open(repo_root, run_id)?;
    let ctx = RunContext {
        run_id: run_id.to_string(),
        mode,
    };
    Ok(dispatch_nodes(repo_root, nodes, &ctx, &sink))
}

/// Dispatch each node through its kernel, then emit its frames. Kernel outcomes
/// are always recorded; frame-emission errors are isolated per node so one bad
/// frame never aborts the run.
pub fn dispatch_nodes(
    repo_root: &std::path::Path,
    nodes: &[DispatchNode],
    ctx: &RunContext,
    sink: &EventSink,
) -> DispatchReport {
    let mut report = DispatchReport::default();
    for node in nodes {
        let (label, frames) = match node {
            DispatchNode::Tool {
                descriptor,
                node_id,
                input,
                incoming_taint,
                action_class,
            } => {
                let granted_offset = sink.next_offset();
                let (outcome, frames) = plan_tool(
                    descriptor,
                    node_id,
                    input,
                    incoming_taint,
                    *action_class,
                    ctx,
                    granted_offset,
                );
                report.tools.push(outcome);
                (format!("tool {node_id}"), frames)
            }
            DispatchNode::Feed { spec, seq } => {
                let (env, frames) = plan_feed(spec, *seq, ctx);
                report.feeds.push(env);
                (format!("feed {}", spec.id), frames)
            }
            DispatchNode::Route {
                route_id,
                policy,
                candidates,
            } => {
                let (winner, frames) = plan_route(route_id, policy, candidates);
                report.routes.push((route_id.clone(), winner));
                (format!("route {route_id}"), frames)
            }
            DispatchNode::Tournament {
                tournament_id,
                candidates,
                judges,
            } => {
                let (outcome, frames) = plan_tournament(tournament_id, candidates, judges);
                report.tournaments.push(outcome);
                (format!("tournament {tournament_id}"), frames)
            }
            DispatchNode::PaperBuilder {
                node_id,
                request,
                workflow_ref,
                source_hash,
                interface_hash,
            } => {
                let (receipt, frames, error) = plan_paper_builder(
                    repo_root,
                    node_id,
                    request,
                    workflow_ref,
                    source_hash,
                    interface_hash,
                    ctx,
                );
                if let Some(receipt) = receipt {
                    report.paper_builds.push(receipt);
                }
                if let Some(error) = error {
                    report
                        .errors
                        .push(format!("paper_builder {node_id}: {error}"));
                }
                (format!("paper_builder {node_id}"), frames)
            }
        };
        for (kind, data) in frames {
            if let Err(err) = sink.emit(kind, data) {
                report.errors.push(format!("{label}: {err}"));
            }
        }
    }
    report
}

/// Tool: authorize (taint / sandbox / capability / credential gates, in the tool
/// kernel) → invoke (gated by `ctx.mode`) → receipt. Returns the outcome + one
/// `ToolCallUpdate` frame per phase (queued|authorizing|denied|started|…).
fn plan_tool(
    descriptor: &ToolDescriptor,
    node_id: &str,
    input: &Value,
    incoming_taint: &TaintSet,
    action_class: ActionClass,
    ctx: &RunContext,
    granted_offset: u64,
) -> (ToolCallOutcome, Vec<(EventKind, Value)>) {
    let receipt_id = format!("rcpt:{}:{node_id}", ctx.run_id);
    let adapter = adapter_for(descriptor);
    let rc = RunToolCtx {
        descriptor,
        node_id,
        input,
        incoming_taint,
        action_class,
        granted_offset,
        receipt_id: &receipt_id,
        source_mode: ctx.mode,
    };
    let outcome = run_tool(&rc, adapter.as_ref());
    let frames = outcome
        .phases
        .iter()
        .map(|phase| {
            (
                EventKind::ToolCallUpdate,
                json!({ "tool_id": descriptor.tool_id, "node_id": node_id, "phase": phase_label(*phase) }),
            )
        })
        .collect();
    (outcome, frames)
}

/// Feed: deterministic fixture tick → provenance/taint-tagged envelope → bounded
/// `FeedTick` frame. The `FixtureSourceProvider` is used in ALL modes in this
/// layer; the envelope's provenance records the (fixture) source so a tick is
/// never mistaken for live data. Live connector polling (`ConnectorSpec::
/// authorize_call` + a real fetch) is the M12-cont network tail, not wired here.
fn plan_feed(
    spec: &DataFeedSpec,
    seq: u64,
    ctx: &RunContext,
) -> (ValueEnvelope, Vec<(EventKind, Value)>) {
    let value = FixtureSourceProvider.tick(spec, seq);
    // observed-at is derived from the sequence (not the wall clock) so the
    // envelope content is reproducible across replays.
    let env = envelope_from_tick(spec, &ctx.run_id, seq, value, seq as i64);
    let label = if spec.symbols.is_empty() {
        spec.id.clone()
    } else {
        spec.symbols.join(",")
    };
    let frame = feed_tick_frame(&env, &label);
    (env, vec![(EventKind::FeedTick, frame)])
}

/// Route: emit the inspectable topology — one decision, one provider-call frame
/// per candidate, then the committed winner. `policy` is emitted for audit /
/// inspection; selection is currently cost-only (see [`pick_route_winner`]) —
/// policy-aware selection (cascade/quorum/…) is a tail.
fn plan_route(
    route_id: &str,
    policy: &str,
    candidates: &[RouteCandidate],
) -> (Option<String>, Vec<(EventKind, Value)>) {
    let mut frames = vec![(
        EventKind::RoutingDecision,
        json!({ "route_id": route_id, "policy": policy, "n": candidates.len() }),
    )];
    for c in candidates {
        frames.push((
            EventKind::ProviderCall,
            json!({ "route_id": route_id, "candidate_id": c.candidate_id, "phase": "started" }),
        ));
    }
    let winner = pick_route_winner(candidates).map(|i| candidates[i].candidate_id.clone());
    if let Some(w) = &winner {
        frames.push((
            EventKind::RouteWinner,
            json!({ "route_id": route_id, "candidate_id": w }),
        ));
    }
    (winner, frames)
}

/// Pick the route winner deterministically: the lowest finite cost, ties broken
/// by the earliest candidate (NaN/∞ treated as worst). Pure + total.
pub fn pick_route_winner(candidates: &[RouteCandidate]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, c) in candidates.iter().enumerate() {
        let cost = if c.cost_usd.is_finite() {
            c.cost_usd
        } else {
            f64::INFINITY
        };
        match best {
            Some((_, b)) if cost >= b => {}
            _ => best = Some((i, cost)),
        }
    }
    best.map(|(i, _)| i)
}

/// Tournament: blind-judge + verify/refute, then emit the generation result +
/// promotion gate (and a blindness-degraded frame if the panel could not be
/// guaranteed independent). The verifier is FAIL-CLOSED by default — a real run
/// MUST inject a deterministic shell/tool check; promotion never succeeds on an
/// un-wired check (green is earned, never assumed).
fn plan_tournament(
    tournament_id: &str,
    candidates: &[Candidate],
    judges: &[JudgeInput],
) -> (TournamentOutcome, Vec<(EventKind, Value)>) {
    let outcome = run_tournament(
        candidates,
        judges,
        |_| VerifierVerdict {
            passed: false,
            detail: "no deterministic verifier wired (fail-closed)".to_string(),
        },
        |_| false,
    );
    let best = outcome.winner_id.clone().unwrap_or_default();
    let mut frames = vec![(
        EventKind::TournamentGeneration,
        json!({ "tournament_id": tournament_id, "gen": 0, "best": best }),
    )];
    if outcome.degraded {
        frames.push((
            EventKind::JudgeBlindnessDegraded,
            json!({ "tournament_id": tournament_id }),
        ));
    }
    frames.push((
        EventKind::PromotionGate,
        json!({ "tournament_id": tournament_id, "promoted": outcome.promoted }),
    ));
    (outcome, frames)
}

// ===========================================================================
// FlowGraph IR → dispatch bridge. Maps a compiled (byte-deterministic) FlowGraph
// IR's executable nodes to `DispatchNode`s and runs them through the kernels.
// This is the seam the `jekko zyal-dispatch` command calls — what turns the
// compiled language into actual gated, receipted, replayable execution.
// ===========================================================================

/// Map ONE compiled IR node to a `DispatchNode`, or `None` if it is not
/// executable from the static IR alone. `nodes` is the full node list so a
/// router can gather its `provider_call` children into route candidates.
///
/// Mapped (fully determined by the IR): `data_feed` → Feed (deterministic
/// fixture tick), `router`/`route_decision` → Route (decision → provider calls →
/// winner). NOT mapped here: `provider_call` (folded into its router), and
/// `tool`/`tournament`/`judge` (need runtime inputs/candidates the static IR
/// lacks) and pure-topology kinds (`agent`/`supervisor`/`spawn`/`watcher`/`kpi`/
/// gates) — they run via other paths. Mirrors `node-types.json` `runtime.executor`.
pub fn node_to_dispatch(node: &Value, nodes: &[Value]) -> Option<DispatchNode> {
    let id = node.get("id")?.as_str()?;
    let node_type = node.get("node_type")?.as_str()?;
    match node_type {
        "data_feed" => {
            let cfg = node.get("data_feed").cloned().unwrap_or(Value::Null);
            Some(DispatchNode::Feed {
                spec: DataFeedSpec::from_value(id, &cfg),
                seq: 0,
            })
        }
        "router" | "route_decision" => {
            let policy = node
                .get("router")
                .and_then(|r| r.get("strategy"))
                .and_then(Value::as_str)
                .unwrap_or("route")
                .to_string();
            // Candidates = the provider_call boxes the router fanned out to
            // (their ids are `<router>/...`). Static cost is uniform (0.0) — the
            // IR carries no per-call cost, so the winner is the earliest (stable).
            let prefix = format!("{id}/");
            let candidates: Vec<RouteCandidate> = nodes
                .iter()
                .filter(|n| n.get("node_type").and_then(Value::as_str) == Some("provider_call"))
                .filter_map(|n| n.get("id").and_then(Value::as_str))
                .filter(|cid| cid.starts_with(&prefix))
                .map(|cid| RouteCandidate {
                    candidate_id: cid.to_string(),
                    cost_usd: 0.0,
                })
                .collect();
            if candidates.is_empty() {
                return None;
            }
            Some(DispatchNode::Route {
                route_id: id.to_string(),
                policy,
                candidates,
            })
        }
        "paper_builder" => {
            let request_value = node.get("paper_builder").cloned().unwrap_or(Value::Null);
            let request: PaperBuildRequest = serde_json::from_value(request_value).ok()?;
            let workflow = node.get("workflow_call").and_then(Value::as_object);
            Some(DispatchNode::PaperBuilder {
                node_id: id.to_string(),
                request,
                workflow_ref: workflow
                    .and_then(|w| w.get("ref"))
                    .and_then(Value::as_str)
                    .unwrap_or(GLOBAL_REF)
                    .to_string(),
                source_hash: workflow
                    .and_then(|w| w.get("source_hash"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                interface_hash: workflow
                    .and_then(|w| w.get("interface_hash"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        }
        _ => None,
    }
}

/// Dispatch a compiled FlowGraph IR's executable nodes through the kernels on the
/// run's real event spine. The entry point the runner/CLI calls after `zyalc`
/// compiles a `target=flowgraph` `.zyal` — `mode` gates live/replay/fake.
pub fn dispatch_flowgraph(
    ir: &Value,
    repo_root: &std::path::Path,
    run_id: &str,
    mode: crate::run_store::SourceMode,
) -> Result<DispatchReport> {
    let empty: Vec<Value> = Vec::new();
    let nodes = ir.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let dispatch: Vec<DispatchNode> = nodes
        .iter()
        .filter_map(|n| node_to_dispatch(n, nodes))
        .collect();
    run_flow_dispatch(repo_root, run_id, mode, &dispatch)
}

fn phase_label(phase: ToolPhase) -> String {
    serde_json::to_value(phase)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
include!("dispatch/tests.rs");
