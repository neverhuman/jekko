# ZYAL Changelog

## Unreleased

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
  `agent/superworkflows/ambitious-superworkflow.superworkflow.json`.
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
    `agent/sandbox-lanes.zyal` → `agent/sandbox-lanes.toml`.
  - Profile C (`target=github-workflow`) — compiles to
    `.github/workflows/*.yml` so GitHub Actions can still find them.
- Shipped the `zyalc` compiler (`crates/zyalc/`) with idempotent
  `compile --all --check` drift detection.
- Added the sandbox-loop declarative function (`agent/sandbox-lanes.toml`)
  with three backends (worktree / bubblewrap / docker) and the
  `sandboxctl` runtime (`crates/sandboxctl/`).
- New jankurai rules: `HLT-032-ZYAL-COMPILE-DRIFT`,
  `HLT-033-UNDECLARED-SANDBOX-LOOP`.

## 2.4.0 - 2026-05-11

- Added the `research` block contract and preview surface for cited external evidence gathering.
- Finalized the contract/version split for ZYAL docs and preview metadata.
- Documented the runtime coverage limits, receipts, and preview-only boundaries for the current research path.
- Kept `.zyal.yml` compatibility strict: existing blocks remain valid unless they introduce unknown keys.
