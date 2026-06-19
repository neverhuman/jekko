# ZYAL Changelog

## Unreleased

### 2026-05-27 - zyal-testing campaign + GOD-level logging follow-up

Live-test campaign on the `zyal-testing` branch validated that ZYAL
pipelines (advanced-reasoning, OpenQG superreasoning, MiniRedis port,
12-stage SuperWorkflow plan-walk) run end-to-end through `jnoccio-fusion`
with `user_1`+`user_2` rotation and recover from upstream failures.
Seven surgical fixes landed; jankurai score floor `70/88/4/7` held
across every commit.

**Fixes (FIX-1..7):**

- **FIX-1** `jekko-runner/src/model_client/runtime.rs`: honor inner
  `jekko run` JSON `"success": true` flag. Stops misclassifying every
  live model call as failed because stderr was non-empty.
- **FIX-2** `scripts/zyal-live-batch.sh`: default fusion to
  `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool` when starting. Prior batches
  silently ran the legacy single-pool path.
- **FIX-3** `zyalc::live_audit`: accept `receipts >= outcomes` (failed
  retries write a receipt but no `model_outcome` event). The prior
  `==` invariant was pre-FIX-1.
- **FIX-4** `zyalc::compile`: status messages to stderr, not stdout.
  Stops chatter leaking into JSON consumers downstream of
  `jekko port-run --dry-run`.
- **FIX-5** `scripts/zyal-live-batch.sh::run_r0`: `unset JEKKO_BIN`
  for the cargo smoke lane. Workaround for the tuiwright matrix tests
  hijacking workspace smoke — proper fix in the follow-up session.
- **FIX-6** `scripts/zyal-live-report.sh`: balancer-no-rotation signal
  distinguishes plan-walk no-op (0 events) from real cursor stall.
- **FIX-7** **BREAKING** —
  `zyal-supervisor::SupervisorStore::init_run` now takes
  `requested_id: Option<&str>` so `jekko port-run --super --run-id <foo>`
  honors `<foo>` end-to-end. External vendors must update.

**Validation:**

- `super-r3-serial` reached `promotion_decision` + `RUN_FINISHED` with
  `verify-replay` passing 5/5 gates.
- Confirmatory rerun (post-FIX-1..7) produced 14 parsed model outcomes
  from 15 attempts (93% parse rate); requested run id honored
  end-to-end.
- 14 distinct upstream providers exercised in `users_pool` mode;
  ~87% aggregate gateway success rate.
- 2 `fusion_model_win_total` increments captured (nvidia, groq) —
  competitive evidence collecting at `routing.fusion_sample_rate: 0.1`.

**Deferred:**

- `MODEL_QUALITY_BAND` (design at `docs/ZYAL/MODEL_QUALITY_BAND.md`)
  — request-side knob to constrain selection by win-rate percentile.
  Heavy MiniRedis halts on empty `stage_brainstorm` responses; the
  band feature is the load-bearing unblocker.
- 12-stage SuperWorkflow walker per-phase LLM work (currently scaffold
  via `port-run --super`).

### 2026-05-26 - Super-Agent operator surfaces (Phase H wave)

- `jekko port-run --super <manifest>` integration wrapper: compiles a
  superworkflow `.zyal` via `zyalc`, validates the DAG, persists state
  to `zyal-supervisor`, and walks execution waves. Phase bodies are
  STUB today; `--live` per-phase invocation is a follow-up. Flags:
  `--dry-run`, `--resume <id>`, `--status <id>`, `--db <path>`,
  `--max-stages <n>`, `--time-budget-hours <h>`.
- `jekko watch <run_id>`: notify-based tail of
  `target/zyal/runs/<run_id>/events.jsonl`. Output formats `plain`
  (default), `json`, `tui`. Surfaces all four remediation rules
  (stall, provider error burst, parity gaps growing, jankurai
  regression) with `--stall-threshold` and `--error-rate-threshold`
  overrides. See `docs/ZYAL/OBSERVABILITY.md`.
- `jnoccio-fusion` `/metrics` Prometheus endpoint:
  `text/plain; version=0.0.4` scrape at the canonical path. 19 metric
  families (gateway totals + per-`{model, provider}` counters and
  per-`{model}` gauges). Mirrors the JSON dashboard at
  `/v1/jnoccio/metrics`. See `docs/ZYAL/OBSERVABILITY.md`.
- `zyalc compile` Profile D — SuperWorkflow: new `target=superworkflow`
  pragma compiles a 9-12 phase manifest to canonical JSON. Reference
  manifest at `agent/zyal/ambitious-superworkflow.zyal` emits
  `generated/superworkflows/ambitious-superworkflow.superworkflow.json`.
- New crates landed under `crates/`:
  - `zyal-core` — canonical types shared across the backbone.
  - `zyal-key-pool` — multi-user credential scan + `PolicyHook` trait
    (default `AlwaysAllow` stub).
  - `zyal-supervisor` — phase planner + 8-table SQLite store driving
    the SuperWorkflow execution waves.
- `jnoccio-fusion` multi-user routing: when
  `JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool`, the gateway fans one
  `ResolvedModel` per `~/.jekko/users/<id>/llm.env` slot keyed by the
  entry's provider env name. Router id becomes
  `{provider}/{model}@{user_id}` so per-user slots route
  independently. `PolicyHook` gate runs `check_and_reserve` before
  upstream dispatch; refusals return HTTP 429. See
  `docs/ZYAL/MULTI_USER_KEYS.md`.

### Earlier unreleased

- Added the canonical human-facing schema at `docs/ZYAL/SPEC.md` together with
  generator/check tooling so parser/schema drift cannot go silent.
- Added three semantic bug-finder runbooks: basic, advanced, and ultra.
- Added three semantic improvement-finder runbooks: simple, advanced, and insane.
- Added three semantic feature-maker runbooks: simple, advanced, and insane.
- Kept the contract version at `2.6.0`; this is a docs/spec-governance update,
  not a schema bump.

## 2.6.0 - 2026-05-12

- Added first-class `jankurai:` syntax for host-enforced audit, repair-plan
  ingestion, task routing, verification, rollback, checkpoint, and branch/main
  regression checks.
- Added bundled examples `jankurai-continuous-repair` and
  `jankurai-porting-advanced`, both requiring `unsupported_feature_policy:
  required: [jankurai]`.
- Kept the runtime sentinel at `v1`; the contract and preview metadata move
  to `2.6.0`.

## 2.5.0 - 2026-05-11

- Renamed canonical extension from `.zyal.yml` to bare `.zyal`. Docs examples
  and paper listings migrate in-place; existing sentinel-wrapped syntax is
  unchanged.
- Introduced two new declarative profiles disambiguated by top-of-file
  pragma:
  - Profile B (`target=toml`) — compiles to TOML, first user is
    `agent/sandbox-lanes.zyal` → `generated/sandbox-lanes.toml`.
  - Profile C (`target=github-workflow`) — compiles to
    `.github/workflows/*.yml` so GitHub Actions can still find them.
- Shipped the `zyalc` compiler (`crates/zyalc/`) with idempotent
  `compile --all --check` drift detection.
- Added the sandbox-loop declarative function (`generated/sandbox-lanes.toml`)
  with three backends (worktree / bubblewrap / docker) and the
  `sandboxctl` runtime (`crates/sandboxctl/`).
- New jankurai rules: `HLT-032-ZYAL-COMPILE-DRIFT`,
  `HLT-033-UNDECLARED-SANDBOX-LOOP`.

## 2.4.0 - 2026-05-11

- Added the `research` block contract and preview surface for cited external evidence gathering.
- Finalized the contract/version split for ZYAL docs and preview metadata.
- Documented the runtime coverage limits, receipts, and preview-only boundaries for the current research path.
- Kept `.zyal.yml` compatibility strict: existing blocks remain valid unless they introduce unknown keys.
