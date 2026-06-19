# ZYAL Agent Playbook

The single entry-point for any future agent or operator working on ZYAL.

If you have only thirty seconds, read **§1 TL;DR** and then jump to the
latest run's `summary.md` under `target/zyal/runs/`.

If you have ten minutes, read **§3 Reading a SUMMARY.json field-by-field**
and **§4 Signals**.

If you're about to implement something, read **§5 Adding a new signal**
or **§6 Adding a fix** depending on which you're doing.

---

## 1. TL;DR

ZYAL is a long-horizon agent-coding substrate. A *run* drives one or more
LLM-backed pipelines through phases (frame → reasoning → parity → judge →
signoff) and emits a forensic event stream (`events.jsonl`) plus a typed
post-run summary (`summary.json` / `summary.md`) at
`target/zyal/runs/<run_id>/`.

State at the time of this writing:
- Live recipes work end-to-end via `JEKKO_ZYAL_LIVE=1 just zyal-{advanced,superreasoning,miniredis}-live-local <rid>`.
- The `jnoccio-fusion` gateway runs in `users_pool` mode, fanning the
  multi-tenant `~/.jekko/users/{user_1,user_2}/llm.env` keys across 14
  providers.
- Heavy MiniRedis runs reach `stage_brainstorm` but halt on empty
  responses. The unblocker is `quality_band: top20` on that stage — see
  `docs/ZYAL/MODEL_QUALITY_BAND.md`.
- The 12-stage SuperWorkflow walker (`port-run --super`) is plan-walk only
  (Phase H scaffold) — per-phase LLM work is the next feature.
- Jankurai score floor: 70/88, caps 4, findings 7 — held across every fix
  committed.

**Latest known-good live run:** look at the most recent `summary.json` under
`target/zyal/runs/` for `terminal_status == "run_finished"`.

## 2. Forensic surfaces — where everything lives

```
~/.jekko/users/<user>/llm.env       → per-user provider keys (user_1, user_2 active)
~/.jekko/users/.balancer.sqlite     → round_robin_cursor (single table, (provider, model) key)
~/.jekko/zyal-supervisor.sqlite     → 8 tables for SuperWorkflow plan-walk state
target/zyal/runs/<run_id>/          → per-run forensic dir
    events.jsonl                    → newline-delimited event stream (the ground truth)
    model_receipts.jsonl            → one row per model attempt (success or failure)
    replay_receipt.json             → 5-gate replay receipt (proof/replay/parity/leak/jankurai)
    superreasoning_packet.json      → frozen-at-end policy + budget contract
    reviewer_packet.json            → human-reviewable rollup (for /jankurai-status etc.)
    claim_ledger.jsonl              → claims surfaced + their evidence
    unsupported_claims.jsonl        → claims that couldn't be backed
    negative_memory.jsonl           → patterns to AVOID in future runs
    STATE.json / STATE.md           → headless run state (status, gate results)
    summary.json / summary.md       → THIS PLAYBOOK — read first, schema zyal.run_summary.v1
target/openqg/hero-judge/<run_id>/  → OpenQG-specific artifacts (hero metrics, judge metrics, lane CSVs)
target/zyal/live-batch-<UTC>-<tag>/ → 8-rung ladder output from scripts/zyal-live-batch.sh
    runs/<rid>.events.jsonl         → per-rung event copies
    balancer/before-<rid>.sql       → balancer state dumps
    balancer/after-<rid>.sql        → ditto
    metrics-snapshots/*.prom        → 30s-interval Prometheus scrapes
    fusion.log                      → fusion stderr for the entire batch
    report.json + report.md         → folded crack-signal scorecard
docs/ZYAL/live-tests/<RUN>.md       → committed per-test forensic reports (this plan's outputs)
```

## 3. Reading a SUMMARY.json field-by-field

Schema: `zyal.run_summary.v1`. Defined in
`crates/jekko-runner/src/run_summary/types.rs`. Built by
`run_summary::build(run_dir)`. Always present after a finalized hero-judge
run; backfill via `jekko port-run --summarize <run_id>` for prior runs.

### Top-level fields

| Field | Why it matters |
|---|---|
| `schema_version` | Always `"zyal.run_summary.v1"` for now. Bump if the shape changes. |
| `run_id` | Matches the directory name and the events' `.run_id` field. |
| `started_at` / `finished_at` / `duration_seconds` | UNIX epoch seconds. `finished_at` is the timestamp of the LAST event, even if the run halted partway. |
| `pipeline` | `"zyal_advanced_port"`, `"zyal_hero_judge"`, `"zyal_miniredis"`, or `"unknown"`. Hint about which manifest produced this run. |
| `terminal_status` | One of `run_finished` (the only good outcome), `halted` (something stopped progress), `budget_exhausted` (max_calls hit), `timeout`. |

### `halt_reason` (only present when terminal_status != run_finished)

This is the SINGLE field a future agent should read first. It tells you:
- **kind** — typed cause (`empty_response_streak`, `final_block`, `budget_exhausted`, `model_failure`, `parse_failure`, …).
- **stage** — which ZYAL stage was active when the halt happened.
- **consecutive_attempts** — how many retries in a row hit the same failure mode.
- **providers_tried** / **users_tried** — what the runner already tried, before halting.
- **summary** — human-readable one-liner with the recommended next step.

Decision tree:

```
halt_reason.kind == "empty_response_streak"  → declare quality_band:top20 on the affected stage
halt_reason.kind == "budget_exhausted"       → raise live_call_budget.max_calls in the manifest
halt_reason.kind == "final_block"            → inspect events.jsonl for the failing stage; raise model-call timeout
halt_reason.kind == "model_failure"          → check fusion.log + check provider health; possibly cool down
```

### `pipeline_progress`

What stages the run reached:
- **deepest_stage** — highest-level stage that emitted a `reasoning_state` event.
- **stages_reached** — every distinct `reasoning_state.data.state`, in order seen.
- **stages_completed** — stages that emitted a `phase_finalized` event.
- **artifacts_produced** — every distinct `reasoning_artifact.data.kind` (task_contract, evidence, context_pack, stage_proposal, …).

### `model_calls`

The headline numbers:
- **total_attempts** — count of `model_attempt` events.
- **parsed_outcomes** — count of `model_outcome` events (only emitted on successful parse).
- **retryable_failures** / **final_blocks** — failure classifications.
- **empty_responses** — count of attempts where `response_bytes == 0`. If this is ≥ 3 you'll see `empty_response_streak` in signals_fired.
- **by_user** — attempts per `credential_user_id` (typically `{user_1: N, user_2: M}` on a healthy run).
- **by_provider** — attempts per upstream provider as fusion reports it.
- **by_kind** — attempts per `ModelTaskKind` (frame, stage_brainstorm, stage_critique, …).
- **by_state** — attempts per outcome state (parsed, retryable_failure, final_block, …).
- **by_quality_band** — attempts per declared quality band (any, top10, top20, top50, bottom20).
- **latency_p50_ms / latency_p95_ms** — over all attempts.

### `budget`

- **max_calls** — from the manifest's `live_call_budget.max_calls`.
- **used** — `live_budget.data.used` from the LAST event of that kind.
- **remaining** — same but `.remaining`.
- **exhausted** — `true` iff `remaining == 0`.

### `balancer`

The round-robin cursor delta for this run:
- **before_cursor** / **after_cursor** — the (`provider`, `model`) cursor value before and after.
- **delta** — `after - before`. Should equal the number of model attempts on a healthy run.

Caller-populated. The builder leaves it 0 unless the caller passes
explicit before/after snapshots; the live recipes' bash drivers do this.

### `signals_fired`

The 12 canonical signals from `OBSERVABILITY.md` + new ones. Format:
`[{id, name, count, evidence?}]`. Only rows with `count > 0` are
interesting. See §4.

### `gates`

`{proof_gate, replay_gate, parity_gate, leak_gate, jankurai_gate}` →
`passed | failed | not_reached`. Drawn from event-stream observations.

### `artifact_paths` / `links`

Pointers to the other files. Open `events_jsonl` first when you want raw
ground truth.

### `operator_next_steps`

Heuristic recommendations keyed off `halt_reason.kind`, `gates` failures,
and `budget.exhausted`. Treat as a starter — not a complete plan.

## 4. Signals — the 12 canonical + new

Canonical list lives in `scripts/zyal-live-report.sh` (the bash that
folds a batch into a report) and is mirrored into SUMMARY.json's
`signals_fired`. Each signal has a `jq` filter, a first-inspection code
path, and a fix template.

| # | Name | Fires when | First inspection | Fix template |
|---|------|------------|------------------|--------------|
| 1 | `model_attempt_outcome_burst` | `attempts >= 20 AND failures/attempts > 0.5` (matches `ProviderErrorBurst`) | `crates/jekko-runner/src/watcher/remediation.rs` | Check provider health; consider cooldown adjustment |
| 2 | `balancer_no_rotation` | cursor unchanged across a run that emitted ≥1 `model_attempt` | `crates/zyal-key-pool/src/balancer.rs` | Check cursor key (provider, model); add user dimension if rotation needs to be per-user |
| 3 | `parity_gap_open_growth` | parity_gap count > parity_result count, ≥ 3 gaps | `crates/jekko-runner/src/parity_lab/gaps.rs` | Escalate to Critique lane — see watcher::ParityGapsGrowing |
| 4 | `worker_stall_or_quarantine` | any `worker_stall` or `worker_quarantine` event | `crates/jekko-runner/src/watcher/mod.rs` | Tune the stall-threshold; investigate heartbeat cadence |
| 5 | `live_budget_exhaustion` | `live_budget.data.remaining <= 0` | `crates/zyal-key-pool/src/budget.rs` | Raise manifest's `live_call_budget.max_calls` |
| 6 | `proof_failed_in_live_lane` | any `proof_failed` event | `crates/jekko-runner/src/stage0_proof/` | Inspect `proof_failed.data.reason` |
| 7 | `provider_error_rate_explosion` | `fusion_model_failure_total / fusion_model_requests_total > 0.5` AND ≥ 20 attempts | `jnoccio-fusion/src/failure_log.rs` + `state.rs` | Check provider keys; consider rotating user_1↔user_2 manually |
| 8 | `latency_outlier_per_provider` | `fusion_model_latency_avg_ms` outlier vs. the pool median | `jnoccio-fusion/src/limits.rs::cooldown_for` | Adjust cooldown for that provider |
| 9 | `jankurai_regression` | `audit_result.data.hard_findings` increased mid-run | `.jankurai/repo-score.md` | Fix the new finding before re-attempting the run |
| 10 | `heartbeat_silence` | gap > 90 s between `heartbeat` events | `crates/jekko-runner/src/superreasoning/runner.rs` | Tune heartbeat cadence; ensure background tasks emit progress events |
| 11 | `parity_result_no_evidence` | `parity_result` event with `evidence_paths == null` | `crates/jekko-runner/src/parity_lab/runner.rs` | Ensure parity adapters emit at least one evidence path |
| 12 | `judge_patch_without_proof` | `judge_patch` event with no matching `proof_passed` within 120 s | `crates/jekko-runner/src/hero_judge_runner_finalize/` | Likely budget-driven — raise budget OR tighten judge→proof loop |
| — | `empty_response_streak` | 3 consecutive `response_bytes == 0` at the same stage | `crates/jekko-runner/src/empty_response_tracker.rs` | **Declare `quality_band: top20`** on the affected stage's model_policy |

## 5. Adding a new signal — recipe

1. **Decide whether you need a new `EventKind` variant.** Often you don't — enrich the `data` payload of an existing event.
2. **If yes:** add the variant to `crates/jekko-runner/src/events.rs` with a doc comment explaining when it fires. The `serde(rename_all = "snake_case")` macro at the top of the enum gives you a `snake_case` JSON name automatically.
3. **Detect & emit** in the appropriate source location. Pattern: a small struct that holds the state (counter, observed providers, …) + a `record()` method that emits exactly once on threshold crossing. See `crates/jekko-runner/src/empty_response_tracker.rs` as the canonical template.
4. **Fold into the WatcherSnapshot** if the signal should affect remediation. Edit `crates/jekko-runner/src/watcher/metrics.rs`; add a field; bump fold logic.
5. **Add a remediation rule** if the signal warrants automatic action: `crates/jekko-runner/src/watcher/remediation.rs::detect_and_remediate`.
6. **Surface in SUMMARY.json.** Edit `crates/jekko-runner/src/run_summary/build.rs::canonical_signal_table` to include the new id/name. The `operator_next_steps` heuristic in the same file can recommend a fix.
7. **Update `scripts/zyal-live-report.sh`** if the signal should appear in the 12-signal batch scorecard.
8. **Document in this playbook (§4) and in `docs/ZYAL/OBSERVABILITY.md`.**
9. **Test.** Unit-test the detector (`empty_response_tracker::tests` is the template). Integration-test that SUMMARY.json carries the signal.

## 6. Adding a fix — the FIX-N pattern

Carried forward from the zyal-testing campaign:

1. **Reproduce.** Run the failing recipe alone; confirm the signal fires deterministically.
2. **Read** the inspection target end-to-end. Don't skim.
3. **Design** a surgical single-concern diff. Prefer reusing existing helpers (`cooldown_for`, `error_rate`, `EventSink::emit`).
4. **Stay under 500 LOC per file.** `crates/jekko-runner/src/reasoning_io.rs` is the dominant score lever at 498 LOC — DO NOT touch beyond the absolute minimum. Use a helper module if the change is non-trivial.
5. **Local test.** `cargo check -p <crate> --locked && cargo test -p <crate> --locked --lib`.
6. **Audit.** `rtk jankurai audit . --mode advisory --json .jankurai/repo-score.json --md .jankurai/repo-score.md`. Assert `final_score >= 70`, `raw >= 88`, no new caps.
7. **Commit.** One logical change per commit. Match the conventional-commit style of `git log`: `fix(<scope>): <one-line>` or `feat(<scope>): <one-line>`.
8. **Rerun & verify** the originally-failing recipe; check `summary.json.halt_reason` shows the signal cleared.
9. **Document** in `docs/ZYAL/live-tests/FIX-<n>.md` (or in the relevant per-run report's appendix).

Hard constraints (jankurai-aligned):
- No new fallback-soup / vibe placeholders in product code.
- No file growing past 500 LOC.
- No `--no-verify` on commits.
- No force-push.
- No editing `agent/zyal/ambitious-superworkflow.zyal` or `docs/ZYAL/examples/*.port-run.json` to "make a run pass" — those are the contracts under test.

## 7. Pipeline catalog

### `advanced_reasoning` (zyal_advanced_port)
- **Recipe:** `just zyal-advanced-reasoning-live-local <rid>`
- **Manifest:** `docs/ZYAL/examples/31-advanced-reasoning-foundry.port-run.json`
- **Stages:** capture_target → frame_request → retrieve_context → brainstorm_stages → critique → reduce → verifier
- **Halt patterns:** brainstorm parse failures (use quality_band:top20) and budget exhaustion (max_calls=12 by default).
- **Artifacts:** task_contract, evidence, context_pack, stage_proposal, critique, reduction.

### `hero_judge` / OpenQG superreasoning (zyal_hero_judge)
- **Recipe:** `just zyal-superreasoning-live-local <rid>`
- **Manifest:** `docs/ZYAL/examples/34-superreasoning-openqg-foundry.zyal`
- **Subcommand:** `hero-judge-run --max-generations N` (not `port-run`)
- **Stages:** research_receipt × N → hero_judge_generation → hero_candidate (lanes) → judge_patch → verifier_score → promotion_decision → run_finished
- **Halt patterns:** non-parseable JSON at literature_synthesis stage (model declines), judge_patch_without_proof on tight budgets.
- **Artifacts:** prompt_lineage, frontier_scoreboard, promotion_decision, knowledge_compound, quality_metrics, lane_metrics, hero_metrics, judge_metrics, reviewer_packet.

### `miniredis_port` (port-run on rust-redis-replacement-superreasoning)
- **Recipe:** `just zyal-miniredis-live-local <rid>`
- **Manifest:** `docs/ZYAL/examples/35-rust-redis-replacement-superreasoning.{zyal,port-run.json}`
- **Stages:** Same as advanced_reasoning but driving a target/replacement parity contract (miniredis ↔ minikv).
- **Halt patterns:** Same as advanced_reasoning. Parity_lab requires reaching at least stage `parallel_subsystems`, which is currently blocked content-side on `stage_brainstorm` for the MiniRedis target.

### `super_workflow_plan_walk` (port-run --super)
- **Recipe:** `just zyal-super-redis <rid>` OR direct `jekko port-run --super <manifest>`
- **Manifest:** `agent/zyal/ambitious-superworkflow.zyal` (12-stage canonical)
- **Status:** Scaffold-only (`Phase H scaffold` per Justfile comment). Phases mark Complete instantly; no per-phase LLM work.
- **Halt patterns:** N/A — the walker always reaches `final_signoff` in scaffold mode.
- **Wiring per-phase LLM work** is FIX-CAND-L — feature scope, not landed in the GOD-logging session.

## 8. The `quality_band` knob

Declared on a per-stage model_policy in a `.zyal` manifest:

```yaml
model_policy:
  routine: { quality_band: top50 }
  power:   { quality_band: top20 }
  critic:  { quality_band: top10 }
  verifier: { quality_band: top10 }
```

Recognised values:
- **`top10`** — only top 10% by observed win-rate. For critical stages.
- **`top20`** — top 20%. Load-bearing reasoning (brainstorm/critique).
- **`top50`** — top half. Default for non-critical reasoning.
- **`any`** — no filter. Current behavior.
- **`bottom20`** — bottom 20%. Use for routine work / non-critical paths so weaker models get exercise (and win-rate evidence keeps accumulating).

Cold-start: a model with < 20 calls is "unranked". Unranked models are
ADMITTED in `top*` bands (so exploration keeps refreshing evidence) and
REJECTED in `bottom20` (so we don't blindly send to fresh models).

Soft-fallback: if the band yields zero candidates after filter, fusion
logs `quality_band yielded zero candidates; falling back to full eligible set` (target `jnoccio_fusion::quality_band`, warn) and proceeds.

The data comes from `model_metrics.win_count / call_count` already collected by the fusion-sample mechanism (`routing.fusion_sample_rate: 0.1`). No new data collection required.

## 9. Open issues + fix candidates queue

Status legend: 🟢 landed, 🟡 deferred, 🔴 known issue.

| ID | Status | Description | Owner / Notes |
|---|---|---|---|
| FIX-1 | 🟢 landed | runtime.rs honors inner JSON `success` flag | `01d1f178b` |
| FIX-2 | 🟢 landed | live-batch.sh default `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool` | `78028c7a4` |
| FIX-3 | 🟢 landed | live-audit accepts receipts >= outcomes | `665228425` |
| FIX-4 | 🟢 landed | zyalc status → stderr | `e70e9b449` |
| FIX-5 | 🟢 superseded | `unset JEKKO_BIN` in run_r0 — replaced by FIX-B | `e70e9b449` |
| FIX-6 | 🟢 landed | report.sh distinguishes plan-walk no-op | `8dbcd5db8` |
| FIX-7 | 🟢 landed (BREAKING) | `SupervisorStore::init_run(manifest, requested_id)` | `d72d568a6` |
| FIX-B | 🟢 landed | tuiwright baseline_matrix dual gate (JEKKO_TUI_CAPTURE) | this session |
| FIX-C | 🟢 landed | s1 burst signal gated on failure-rate | this session |
| FIX-D | 🟢 landed | EmptyResponseStreak detector + signal | this session |
| FIX-E | 🟢 landed | MODEL_QUALITY_BAND end-to-end | this session |
| FIX-F | 🟢 landed | GOD-level SUMMARY.json + backfill | this session |
| FIX-CAND-M | 🟢 landed | manifest→runner→jekko-run quality_band plumbing (heavy MiniRedis unblocker) | this session, `179388e16` |
| FIX-CAND-N | 🟢 landed | echo `quality_band` in receipt + outcome event; verified `by_quality_band: {top10:3, top20:2}` populates on heavy-mini-qb-R run | this session |
| FIX-CAND-O | 🟢 resolved | finalize_master_plan uses `ModelTaskKind::StageReduce` → `power` role (already top20); post-band runs show finalize completing with `phase_finalized` event (stage_count=4, task_count=4) | no separate fix needed |
| FIX-CAND-P | 🟡 deferred | bump `live_call_budget.max_calls` from 12 to ~16 in heavy MiniRedis manifest to exercise deeper progress past verifier | 1-line manifest edit |
| FIX-CAND-Q | 🟢 landed | heavy MiniRedis verifier-stage parse failures: tightened verifier prompt to strict JSON schema in finalize.rs. | this session |
| Caps/findings sweep | 🟢 partial | raw 88→91, caps 4→3, findings 7→4: ci-bad-behavior CLEARED, 2 fallback-soup sites + sentinel restructured + web surface removed. fallback-soup-in-product-code cap (70) holds the floor — 55+ remaining sites mostly Rust idiom; clearing requires bulk sweep. agent-tool-supply-chain-gap + missing-rendered-ux-qa-lane caps are jankurai detector-binding issues. | this session |
| Cleanup pass | 🟡 in progress | stale `packages/ux-qa` lane dependency removed, `run_summary` folding split into `fold.rs`, and the serious live-test report is now checked in; final audit/copy-code reruns still pending after the local `jankurai` update. | follow-up cleanup |
| FIX-CAND-L | 🟡 deferred | wire jekko-runner per phase in port-run --super --live | Future feature |
| 12-stage walker scaffold | 🔴 known | `port-run --super` doesn't drive real LLM work yet | Tracked by FIX-CAND-L |

## 10. Glossary

- **parity_lab** — stage that runs N parity test-cases against a reference adapter and a candidate adapter, comparing outputs. Failing cases become `parity_gap` events.
- **hero-judge** — multi-lane reasoning pattern. Hero lanes generate candidates; judge lanes patch them; verifier scores; promotion_decision selects the winner.
- **quality_band** — request-side knob that constrains routing to a percentile slice of models by observed win-rate.
- **users_pool** — fusion routing mode where each `~/.jekko/users/<id>/llm.env` becomes an independent slot. Activated by `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool`.
- **fusion_sample** — the 10%-rate competitive routing where a primary call is duplicated to a backup model and the winner increments `model_metrics.win_count`. This is the data source the quality_band feature consumes.
- **EventKind** — the 44-variant enum at `crates/jekko-runner/src/events.rs` that types every line in events.jsonl.
- **WatcherSnapshot** — the in-memory rollup of an events.jsonl, computed by `fold_events`. Consumed by remediation rules.
- **12-stage canonical kernel** — `agent/zyal/ambitious-superworkflow.zyal`'s phase DAG: source_of_truth → architecture_blueprint → repo_graph_bootstrap → contracts_and_slices → parallel_subsystems → parity_lab → integration_fusion → parity_gap_closure → hardening_security → performance_closure → docs_release_ops → final_signoff.
- **jankurai gate** — `rtk jankurai audit . --mode advisory` must report `final_score >= 70`, `raw >= 88`, no new caps applied. Run before EVERY commit.
- **replay receipt** — `replay_receipt.json` — frozen 5-gate (proof/replay/parity/leak/jankurai) status that anchors the run for future verification.
- **claim ledger** — `claim_ledger.jsonl` — every claim the run surfaced with its supporting evidence path.
- **unsupported claims** — `unsupported_claims.jsonl` — claims that couldn't be backed by evidence; surfaced for reviewer attention.
- **negative memory** — `negative_memory.jsonl` — patterns the run learned to AVOID; carried forward to future runs.

## 11. Common operator commands (cheatsheet)

```bash
# Build the release binary the live recipes pick up
cargo build --release -p jekko-cli -p jekko-runner --locked

# Start fusion in users_pool mode (the only correct mode for live tests)
cd jnoccio-fusion && JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool RUST_LOG=info,jnoccio_fusion=debug ./target/debug/jnoccio-fusion --config config/server.json --env-file .env.jnoccio &
curl -sf 127.0.0.1:4317/health | python3 -m json.tool  # confirm user_count: 2

# One isolated live run — replace recipe name as needed
JEKKO_ZYAL_LIVE=1 JEKKO_BIN=$PWD/target/release/jekko just zyal-superreasoning-live-local my-run-1
JEKKO_ZYAL_LIVE=1 JEKKO_BIN=$PWD/target/release/jekko just zyal-advanced-reasoning-live-local my-run-2
JEKKO_ZYAL_LIVE=1 JEKKO_BIN=$PWD/target/release/jekko just zyal-miniredis-live-local my-run-3

# Live tail
jekko watch my-run-1 --format json --follow

# Plan-walk for the 12-stage canonical kernel (no LLM)
just zyal-super-redis my-walk-1

# Read the post-run summary
cat target/zyal/runs/my-run-1/summary.md
cat target/zyal/runs/my-run-1/summary.json | jq .halt_reason

# Backfill summary.json for a prior run that pre-dates the feature
jekko port-run --summarize my-old-rid

# Full 8-rung ladder + report
BATCH_TAG=campaign-1 JEKKO_ZYAL_LIVE=1 JEKKO_BIN=$PWD/target/release/jekko bash scripts/zyal-live-batch.sh
bash scripts/zyal-live-report.sh target/zyal/live-batch-<UTC>-campaign-1

# Pre-commit audit (always run)
rtk jankurai audit . --mode advisory --json .jankurai/repo-score.json --md .jankurai/repo-score.md
```

## Cross-references

- `docs/ZYAL/SPEC.md` — canonical schema spec.
- `docs/ZYAL/OBSERVABILITY.md` — operator surfaces (`jekko watch`, `/metrics`).
- `docs/ZYAL/MODEL_QUALITY_BAND.md` — quality-band feature design.
- `docs/ZYAL/MULTI_USER_KEYS.md` — multi-user key pool.
- `docs/ZYAL/PORT_WORKFLOW.md` — port-run workflow details.
- `docs/ZYAL/SUPER_REASONING_WORKFLOWS.md` — 12-stage canonical SuperWorkflow.
- `docs/ZYAL/live-tests/` — per-run + per-batch forensic reports.
- `AGENTS.md` (repo root) — top-level agent instructions.
