# Testing

Jekko is a Rust workspace. Tests run through `cargo` or `xtask` (the
`crates/xtask` automation crate); proof lanes are wrapped in `just` recipes.

Pick the narrowest lane that matches the change. Quick lanes give fast
feedback during iteration; comprehensive lanes are for pre-merge and release
gating.

## Quick lanes

### `just fast`

Narrow workspace lane: typecheck, build, and the fast subset of unit tests.
Internally: `cargo check --workspace --locked`, `cargo build --workspace
--locked`, then targeted `cargo test` lanes on `jekko-core` and `jekko-cli`.

- **Covers:** workspace shape, fast unit tests on core + CLI.
- **Pass:** exit `0`, final line `test result: ok`.
- **Fail:** non-zero exit, `test result: FAILED`.
- **Artifacts:** none beyond `target/`.

Use for documentation changes and small Rust edits before pushing.

### `just tui-startup-smoke`

The fastest gate for the host binary. Builds `jekko-cli`, then runs the PTY
first-frame regression `default_tui_paints_first_frame` from
`crates/tuiwright-jekko-unlock`. Fails fast if the screen is blank after 5 s
or if the home prompt sentinel does not appear within 10 s.

- **Covers:** host binary boot, plugin loader, first paint.
- **Pass:** `test default_tui_paints_first_frame ... ok`.
- **Fail:** PTY timeout or sentinel-not-found.
- **Artifacts:** `target/tuiwright-jekko/boot/*.png`,
  `target/tuiwright-jekko/traces/*.trace.jsonl`,
  `target/tuiwright-jekko/logs/*.log`.

Run first after touching startup, plugin loading, or `.jekko/plugins`.

## Comprehensive lanes

### `cargo test --workspace --locked --no-fail-fast`

Full unit and integration suite across every workspace crate.

- **Covers:** every `#[test]` and integration test in the workspace.
- **Pass:** every package finishes with `test result: ok`.
- **Fail:** any `FAILED` line; `--no-fail-fast` keeps running, so check the
  summary at the bottom.
- **Artifacts:** none beyond `target/`.

## Release Gate Evidence

These checks are the minimum launch evidence for a release candidate:

- Security: secret scanning ran on the workspace and no new credentials were introduced.
- Backups: restore procedure for any persistent state has been exercised on the release branch.
- Monitoring: alerting for the host service and core job queues is configured and green.
- Rollback: the deployment can be reverted to the last known-good artifact without manual repair.
- Abuse controls: request limits, auth checks, and kill switches are documented and tested.

If any of the items above are missing, the release lane should stop and the gap should be repaired before shipping.

## Spend Guardrails

For paid or otherwise unbounded operations, define the spend cap and the stop condition before launching the job:

- Spend cap: the maximum allowed spend for the run.
- Stop condition: the exact signal that ends the job early.
- Kill switch: the operator action that halts the lane if the spend cap is reached.

The release lane should not proceed until the spend-cap and stop-condition evidence are recorded alongside the run receipt. The machine-readable
declaration the release lane consumes lives in [Release budget gate](#release-budget-gate); the per-surface inventory it audits against lives in
[Cost budget proof](#cost-budget-proof) and is mirrored in full at
[`docs/cost-budgets.md`](./cost-budgets.md).

## Release budget gate

This block is the machine-readable budget declaration the release lane
checks before promoting a build. It is the canonical answer to "what is the
release-wide spend ceiling and who can stop the lane if it is breached?"

```json
{
  "lane": "release",
  "rule_id": "HLT-026-COST-BUDGET-GAP",
  "budget_usd": 0,
  "currency": "USD",
  "scope": "default cargo test --workspace --locked invocation across all CI workflows",
  "kill_switch": "manual",
  "kill_switch_owner": "release operator",
  "kill_switch_action": "cancel the GitHub Actions run via the workflow UI or `gh run cancel <run-id>`; locally interrupt with Ctrl-C",
  "approval_ref": "none-yet",
  "approval_note": "no automated dollar-cap enforcement exists in code today; the release lane treats any paid surface as manual-attended until a `JEKKO_PROVIDER_BUDGET_CENTS` guard lands",
  "stop_condition": "any paid surface invoked from CI; any test that initiates a real provider HTTP request without operator opt-in (`JEKKO_TUI_LIVE_PROD=1` and `JEKKO_API_KEY` set, plus `CI != true`)",
  "evidence_paths": [
    "docs/cost-budgets.md",
    ".github/workflows/test.yml",
    "crates/tuiwright-jekko-unlock/tests/live_prod_tui.rs"
  ]
}
```

The release lane MUST NOT proceed if any surface in the [Cost budget
proof](#cost-budget-proof) inventory below is missing a `cost_usd` and
`stop_condition`, or if a paid surface is invoked from CI.

## Cost budget proof

This section is the budget proof for every test surface that could plausibly
incur real spend. It is the canonical answer to "what stops a `cargo test`
from burning provider credit?" The standalone artifact lives at
[`docs/cost-budgets.md`](./cost-budgets.md) and is the single source of
truth; the JSON inventory and table below are mirrors kept in sync for
readers landing in this file first.

```json
{
  "rule_id": "HLT-026-COST-BUDGET-GAP",
  "doc_url": "docs/cost-budgets.md",
  "default_test_command": "cargo test --workspace --locked",
  "default_test_cost_usd": 0,
  "default_test_stop_condition": "no real provider HTTP is wired into any default test path; all adapter tests bind a loopback `tokio::net::TcpListener` and serve canned SSE bytes from `crates/jekko-provider/tests/fixtures/*.sse`",
  "surfaces": [
    {
      "name": "default-unit-integration-suite",
      "command": "cargo test --workspace --locked",
      "cost_usd": 0,
      "currency": "USD",
      "quota": "unbounded test count; zero outbound paid bytes",
      "stop_condition": "no production code path is exercised against a real provider endpoint; if a test ever issues an outbound paid request, the `rg \"reqwest|\\.send\\(\\)\\.await\" crates/jekko-provider/tests/` verification check fails the audit",
      "kill_switch": "manual",
      "kill_switch_action": "Ctrl-C locally; `gh run cancel` in CI",
      "approval_ref": "none-yet"
    },
    {
      "name": "live-provider-tui-proof",
      "command": "cargo test -p tuiwright-jekko-unlock --locked -- --ignored live_jekko_prompt_round_trips_through_tui",
      "cost_usd": 0,
      "cost_usd_ci": 0,
      "cost_usd_local_estimate": 0.0005,
      "currency": "USD",
      "quota": "single round-trip per invocation; one fixed prompt; nano-tier default model `jekko/gpt-5-nano`",
      "stop_condition": "test is `#[ignore]`d, requires `JEKKO_TUI_LIVE_PROD=1` and `JEKKO_API_KEY`, and aborts when `CI=true`; per-test wall timeout is 180 s (`LIVE_TIMEOUT`)",
      "kill_switch": "manual",
      "kill_switch_action": "operator Ctrl-C or the 180 s `LIVE_TIMEOUT` wall clock",
      "approval_ref": "none-yet",
      "approval_note": "no automated `JEKKO_PROVIDER_BUDGET_CENTS` guard; operator-attended only"
    },
    {
      "name": "ci-unit-job",
      "command": ".github/workflows/test.yml::unit",
      "cost_usd": 0,
      "quota": "60 min wall-clock per job; single-runner matrix (linux only)",
      "stop_condition": "`timeout-minutes: 60` and concurrency-group cancellation of stale runs in `.github/workflows/test.yml`",
      "kill_switch": "manual",
      "kill_switch_action": "`gh run cancel <run-id>` or workflow UI cancel",
      "approval_ref": "none-yet"
    },
    {
      "name": "ci-tui-job",
      "command": ".github/workflows/test.yml::tui",
      "cost_usd": 0,
      "quota": "60 min wall-clock per job; single ubuntu-latest runner",
      "stop_condition": "`timeout-minutes: 60` on the `tui` job in `.github/workflows/test.yml`",
      "kill_switch": "manual",
      "kill_switch_action": "`gh run cancel <run-id>` or workflow UI cancel",
      "approval_ref": "none-yet"
    },
    {
      "name": "other-ci-workflows",
      "command": ".github/workflows/{generate,stats,parity,nix-hashes,security,jankurai,deploy}.yml",
      "cost_usd": 0,
      "quota": "10-60 min wall-clock per job (see individual workflow files)",
      "stop_condition": "`timeout-minutes:` declared on every job; no workflow runs paid LLM traffic",
      "kill_switch": "manual",
      "kill_switch_action": "`gh run cancel <run-id>` or workflow UI cancel",
      "approval_ref": "none-yet"
    }
  ],
  "future_work": [
    "Add a `JEKKO_PROVIDER_BUDGET_CENTS` env var honoured by the live-provider lane that aborts the test if predicted spend exceeds the cap.",
    "Emit a structured cost receipt (provider, model, input/output tokens, estimated dollar cost) appended to `target/jankurai/cost-receipts.jsonl` at the end of every `live_jekko_*` run.",
    "Wire the receipt into the release-gate evidence bundle described in `docs/release.md`."
  ]
}
```

See [`docs/cost-budgets.md`](./cost-budgets.md) for the full prose budget
proof, including the verification recipe a reviewer can run from a clean
checkout to confirm the enforcement above is still wired up.

| Surface | Budget | Enforcement | Verification |
|---------|--------|-------------|--------------|
| Default unit + integration suite (`cargo test --workspace --locked`) | $0 / run | No real provider HTTP. Adapter tests in `crates/jekko-provider/tests/` use canned SSE bytes from `tests/fixtures/*.sse` served over a one-shot `tokio::net::TcpListener` bound to `127.0.0.1:0`. | `rg "reqwest\|\.send\(\)\.await" crates/jekko-provider/tests/` returns zero hits; `rg "#\[ignore\]" crates/jekko-provider/` returns zero hits because there are no paid tests in this crate to gate. |
| Live-provider TUI proof (`live_prod_tui.rs`) | $0 in CI; bounded by operator wallet locally | `#[ignore]` on the test, requires `JEKKO_TUI_LIVE_PROD=1`, `require_env("JEKKO_API_KEY")`, and explicitly aborts with an error if `CI=true`. Default model pinned to `jekko/gpt-5-nano` (cheap nano-class). Prompt is a single fixed-string round-trip; per-test wall timeout 180 s. | `rg "JEKKO_TUI_LIVE_PROD\|CI.*true" crates/tuiwright-jekko-unlock/tests/live_prod_tui.rs`; default `cargo test` skips the test (it is `#[ignore]`d). |
| CI runtime | 60 min / job hard cap; single linux matrix entry on the unit + tui workflows | `timeout-minutes: 60` on every job in `.github/workflows/test.yml`; the `matrix.settings` list has one entry (`linux`), so no parallel fan-out spend. Concurrency group cancels stale runs in-progress. | `grep timeout-minutes .github/workflows/test.yml`; `grep -A3 "matrix:" .github/workflows/test.yml`. |

The default `cargo test --workspace --locked` invocation incurs $0 in
provider cost because:

- No production code path is exercised against a real provider endpoint
  during the default test pass.
- The only tests that hit a real provider live under
  `crates/tuiwright-jekko-unlock/tests/live_prod_tui.rs` and are
  `#[ignore]`d *and* refuse to start when `CI=true` *and* require
  `JEKKO_TUI_LIVE_PROD=1` + `JEKKO_API_KEY` to be set.
- No `JEKKO_PROVIDER_BUDGET_CENTS`-style hard dollar cap currently exists
  in the codebase. The current spend ceiling is operator-side: the model is
  pinned to a nano-tier default, the prompt is fixed, and the run is
  single-shot. A per-run dollar cap is tracked as future work; until then
  the kill switch is the operator's `Ctrl-C` and the test's 180 s wall
  timeout.

To run a live-provider smoke locally (paid by the operator):

1. Build the host binary: `cargo build -p jekko-cli --locked`.
2. Export `JEKKO_API_KEY`, `JEKKO_BIN=$(cargo run -p xtask -- host-binary-path)`,
   `JEKKO_TUI_LIVE_PROD=1`. Optionally pin `JEKKO_LIVE_MODEL` to a cheaper
   model.
3. `cargo test -p tuiwright-jekko-unlock --locked -- --ignored
   live_jekko_prompt_round_trips_through_tui`.

If you discover a new cost-bearing surface, add a row to the table above in
the same PR — the audit gate (`HLT-026-COST-BUDGET-GAP`) treats an
undeclared paid surface as a release-blocking gap.

### `just tui-ci`

CI-safe TUI lane. No production keys, no browser. The canonical workflow
body lives in `bash ops/ci/test-tui.sh`; `just tui-ci` is the local wrapper.
Internally: builds the host binary, verifies `jekko --version` and `--help`,
runs `cargo test -p jekko-tui`, compiles every `tuiwright-jekko-unlock`
test, and runs the CI-safe PTY first-frame regression with `JEKKO_BIN`
pointed at the built host binary (via `xtask host-binary-path`).

- **Covers:** TUI crate units, host binary smoke, first-frame PTY regression,
  compile-check of every PTY test.
- **Pass:** every step exits `0`.
- **Fail:** any step fails; lane stops on first failure.
- **Artifacts:** as in `just tui-startup-smoke`.

See `docs/testing-tui.md` for the full TUI lane reference and the baseline
matrix.

## xtask parity gates

These run via `cargo run -p xtask -- <subcommand>`. They produce structured
reports under `target/` and are the parity gates for the Rust port.

### `cargo run -p xtask -- baseline-diff`

Diffs Rust render snapshots against the checked-in baseline.

- **Covers:** TUI render parity across 11 screens × 5 resolutions.
- **Inputs:** `target/tuiwright-jekko/baseline/`,
  `target/tuiwright-jekko/rust/`.
- **Pass:** every pair under `--threshold` (when provided), exit `0`.
- **Fail:** any pair over threshold, exit `1`.
- **Output:** text table by default; `--format json` for machine output.

### `cargo run -p xtask -- db-migration-smoke`

Walks the SQLite migration journal under `db/migrations/` and asserts every
migration applies cleanly on a fresh database.

- **Covers:** `jekko-store` schema and migration ordering.
- **Pass:** exit `0`, all migrations applied.
- **Fail:** non-zero exit with the failing migration filename in stderr.

### `cargo run -p xtask -- httpapi-parity`

Asserts the Axum HTTP routes in `jekko-server` still match the published
API contract (request/response shapes, status codes, error envelopes).

- **Covers:** HTTP contract surface (`jekko-server` routes).
- **Pass:** exit `0`. **Fail:** non-zero with the diverging route in stderr.

### `cargo run -p xtask -- openapi-check`

Regenerates the OpenAPI schema from the live Axum router and compares it
against the checked-in copy.

- **Covers:** OpenAPI schema drift.
- **Pass:** schemas byte-identical, exit `0`.
- **Fail:** non-zero exit with diff in stderr.

### `cargo run -p xtask -- tool-schema-parity`

Compares JSON Schemas emitted for every Rust tool against checked-in
fixtures.

- **Covers:** tool schema stability for the agent runtime.
- **Pass:** every tool schema matches its fixture.
- **Fail:** non-zero with the diverging tool name in stderr.

### `cargo run -p xtask -- session-fixture-parity`

Replays canonical session fixtures through the Rust runtime and checks the
resulting event/log shapes match the checked-in expectations.

- **Covers:** session bus + transcript shapes.
- **Pass:** exit `0`. **Fail:** non-zero with a path to the divergent fixture.

### `cargo run -p xtask -- guard-forbidden-runtime`

Forbidden-token guard. Scans the working tree for tokens that should be gone
from the Rust target. Runs in `--mode advisory` by default; pass `--mode
final` to fail on any hit.

- **Covers:** absence of legacy runtime references in the active tree.
- **Pass:** zero hits in `final` mode, or advisory-only output.
- **Fail:** `--mode final` with any hit, exit `1`.

## Receipt convention

Every proof lane leaves a receipt next to the task that invoked it: the
exact command, exit code, UTC timestamp, output path or summary, and
touched files. Comprehensive lanes additionally record spend-cap and
launch-gate evidence; see `docs/release.md`.

## Observability and repair receipts

This section is the machine-readable observability declaration the audit
gate (`HLT-017-OPAQUE-OBSERVABILITY`) checks against. It is the canonical
answer to "when a test, build, or audit fails, where does the next agent
look to (a) read the structured error, (b) inspect the runtime telemetry,
and (c) get the exact rerun command?"

The Rust workspace uses `thiserror` typed errors per crate, `tracing` for
runtime telemetry routed through `tracing_subscriber::EnvFilter`, and the
auditor (`jankurai`) writes structured repair receipts under
`target/jankurai/` with a `rerun_command` field on every finding.

```json
{
  "rule_id": "HLT-017-OPAQUE-OBSERVABILITY",
  "docs_url": "docs/testing.md#observability-and-repair-receipts",
  "repair_hint": "rerun `just score` after the scoped change, then inspect `target/jankurai/repo-score.json`",
  "rerun_command": "just score",
  "structured_errors": {
    "pattern": "thiserror per-crate enums with typed variants and constructors",
    "result_alias": "<Crate>Result<T> = Result<T, <Crate>Error>",
    "paths": [
      "crates/jekko-runtime/src/error.rs",
      "crates/jekko-provider/src/error.rs",
      "crates/jekko-core/src/error.rs",
      "crates/jekko-server/src/error.rs",
      "crates/jekko-store/src/error.rs",
      "crates/jekko-plugin-api/src/error.rs"
    ]
  },
  "telemetry": {
    "library": "tracing + tracing_subscriber",
    "init": "crates/jekko-cli/src/runtime.rs::bootstrap",
    "env_filter": "RUST_LOG (read by tracing_subscriber::EnvFilter::try_from_default_env)",
    "cli_flags": [
      "--log-level <level>",
      "--print-logs"
    ],
    "default_level": "info (full output requires --print-logs; otherwise warn+ to stderr)",
    "event_bus": {
      "module": "crates/jekko-runtime/src/bus.rs",
      "subscribe_all": "Bus::subscribe_all() -> broadcast::Receiver<EventEnvelope>",
      "subscribe_typed": "Bus::subscribe(kind).await",
      "publish": "Bus::publish(kind, properties).await"
    }
  },
  "repair_receipts": {
    "audit_json": "target/jankurai/repo-score.json",
    "audit_md": "target/jankurai/repo-score.md",
    "repair_queue": "target/jankurai/repair-queue.jsonl",
    "score_history": "target/jankurai/score-history.jsonl",
    "sarif": "target/jankurai/jankurai.sarif",
    "summary": "target/jankurai/summary.md",
    "artifact_paths": [
      "target/jankurai/repo-score.json",
      "target/jankurai/repo-score.md",
      "target/jankurai/repair-queue.jsonl",
      "target/jankurai/score-history.jsonl",
      "target/jankurai/jankurai.sarif",
      "target/jankurai/summary.md"
    ],
    "docs_url": "docs/testing.md#observability-and-repair-receipts",
    "repair_hint": "rerun `just score` after the scoped change, then inspect `target/jankurai/repo-score.json`",
    "rerun_command": "just score",
    "finding_fields": [
      "rule_id",
      "rerun_command",
      "docs_url",
      "agent_fix",
      "evidence",
      "fingerprint",
      "path"
    ]
  },
  "tui_artifacts": {
    "boot_screenshots": "target/tuiwright-jekko/boot/*.png",
    "frame_traces": "target/tuiwright-jekko/traces/*.trace.jsonl",
    "logs": "target/tuiwright-jekko/logs/*.log"
  }
}
```

### Rerun commands by failure mode

The auditor stamps a `rerun_command` on every finding in
`target/jankurai/repo-score.json`. The table below is the canonical
mapping a reviewer can use without parsing JSON; when in doubt, prefer the
`rerun_command` from the finding itself (it is the source of truth and is
keyed to the rule that fired).

| Failure mode | Canonical rerun | Source of truth |
|--------------|-----------------|-----------------|
| Cargo unit/integration test | `cargo test -p <crate> --locked <test_name>` | The `FAILED` line in `cargo test` stderr names the test path. |
| Workspace test sweep | `cargo test --workspace --locked --no-fail-fast` | Used by `just fast` and CI `unit` job. |
| Clippy lint | `cargo clippy -p <crate> --all-targets --all-features -- -D warnings` | `.github/workflows/test.yml` typecheck/lint matrix. |
| Audit / shape rule | `just score` (advisory) or `just audit-ci` (ratchet) | `Justfile::score` writes `agent/repo-score.json`; per-finding `rerun_command` takes precedence. |
| Audit fast lane | `just score-fast` | `Justfile::score-fast` writes `target/jankurai/repo-score.json` with no history side-effects. |
| TUI first-frame regression | `just tui-startup-smoke` | Artifacts under `target/tuiwright-jekko/`. |
| TUI CI lane | `just tui-ci` | See `docs/testing-tui.md`. |
| Live-provider TUI smoke | `JEKKO_TUI_LIVE_PROD=1 JEKKO_API_KEY=… cargo test -p tuiwright-jekko-unlock --locked -- --ignored live_jekko_prompt_round_trips_through_tui` | See [Cost budget proof](#cost-budget-proof). |
| Doctor sweep | `jankurai doctor --fail-on critical` | `target/jankurai/doctor.json` + `target/jankurai/doctor.md`. |

To reproduce the exact audit pass that surfaced the finding:

```sh
jankurai audit . \
  --mode advisory \
  --json target/jankurai/repo-score.json \
  --md target/jankurai/repo-score.md

# Then inspect a single rule's findings (HLT-017 example):
jq '[.findings[] | select(.rule_id == "HLT-017-OPAQUE-OBSERVABILITY")]' \
  target/jankurai/repo-score.json
```

### Reading a repair receipt

Each finding in `target/jankurai/repo-score.json` carries enough state for
an agent to act without re-running discovery:

- `rule_id` — the rule that fired (e.g. `HLT-017-OPAQUE-OBSERVABILITY`).
- `path` — the file or directory the rule is keyed to.
- `problem` — the human-readable failure description.
- `agent_fix` — the fix the rule expects.
- `evidence` — the structured evidence the rule collected.
- `rerun_command` — the exact command that re-evaluates the rule.
- `docs_url` — the standard section that documents the rule.
- `repair_hint` — the short local action the next agent should take.
- `fingerprint` — sha256 of the finding, stable across runs while the
  finding is unresolved (use it to dedupe across history).

The `repair-queue.jsonl` is the same data flattened to one finding per
line for streaming consumers (the audit-gate script and CI summary).

### What this section does not claim

- There is no `ops/observability/` directory in this workspace and no
  external metrics shipper (Prometheus, OTLP, etc.) is wired in. Telemetry
  is in-process `tracing` events plus the in-process event bus.
- There is no `JEKKO_LOG` env var; log level is controlled via `RUST_LOG`
  (read by `tracing_subscriber::EnvFilter`) or the `--log-level` /
  `--print-logs` CLI flags wired in `crates/jekko-cli/src/cli.rs`.
- Repair receipts are written by `jankurai`, not by `cargo test`. A failing
  test still prints to stderr; the audit step is what wraps that into a
  structured finding with a `rerun_command`.
