# ZYAL Port Workflow

ZYAL port workflows are durable daemon runs for building a replacement implementation that matches a reference target. The workflow is generic: Redis/Jedis is one possible target/replacement pair, but the same loop applies to Postgres, Kafka, a tiny fixture, or another system with a discoverable contract.

## Contract

The port surface captures:

- `target`, `replacement`, `target_repo`, and `replacement_repo`
- `phase_strategy`, `worker_cap`, `jankurai_gate`, `repo_graph`, `parity_lab`, and `model_policy`
- stop conditions for budget expiry, dirty primary worktrees, destructive target-repo operations, Jankurai regressions, and quarantine

Runtime state is persisted in SQLite tables for targets, phases, tasks, parity cases, parity runs/results, perf budgets, repo graph nodes/edges, and model outcomes. Runtime artifacts stay under ignored paths such as `.zyal/`, `target/zyal/`, and `.jankurai/`.

Advanced reasoning adds durable tables for reasoning artifacts, artifact edges, reasoning lanes, verified/rejected memory capsules, and per-model reliability. The runner stores structured summaries and payloads only; raw chain-of-thought is redacted by default. Any artifact without executable evidence is confidence-capped at `0.35`, and permanent memory writes require verifier/reducer approval through a verified or rejected capsule.

## Loop

1. Capture the target request and repositories.
2. Brainstorm candidate stages from target docs, source, tests, examples, and repo graph summaries.
3. Finalize an ordered master plan with phases and task ownership.
4. For each phase, draft a phase plan, assign disjoint worker tasks, verify proofs, run Jankurai, and record receipts.
5. Heal cross-phase integration until the replacement builds and runs as one system.
6. Run target-switched parity and performance cases. Missing, skipped, failed, or perf-less required cases block completion.
7. Spawn bounded follow-up tasks for each correctness or perf gap until the parity gate passes.

The advanced state machine is:

```text
capture_target -> frame_request -> retrieve_context -> brainstorm_stages
-> critique_stages -> finalize_master_plan -> track_stage -> brainstorm_phase
-> finalize_phase_plan -> build_phase -> verify_phase -> heal_integration
-> generate_parity -> close_parity_perf -> complete
```

Invalid JSON from a live advanced model call is retried twice and then blocks the run with the parse error recorded in SQLite. Fake deterministic runs may synthesize a structured fallback so local tests do not spend provider budget.

## Safety Rules

- Primary worktree must be clean unless the run explicitly allows dirty.
- Workers use `.zyal/worktrees/<run_id>/<worker_id>` and scoped `zyal/*` branches.
- Worker write scopes must be declared and may not target generated or read-only zones.
- Checkpoints require proof lanes, Jankurai gate evaluation, rollback receipts, and event logs.
- Human approval is required for budget renewal, destructive target-repo operations, and final acceptance.

## Parity Report

Parity cases run against either reference or candidate by switching adapter configuration. Required or approved cases fail the gate when absent, skipped, failed, or missing required performance data. Performance budgets are hard gates: a candidate/reference latency ratio over the case budget is a parity failure and produces a follow-up gap task.

```toml
id = "protocol.ping.basic"
tags = ["protocol", "required", "approved"]
target_kind = "redis-compatible"
steps = [
  { send = "*1\r\n$4\r\nPING\r\n", expect = "+PONG\r\n" }
]
perf = { p95_ms_max_ratio = 1.25 }
```

## Receipts

Every run should emit bounded NDJSON events plus SQLite rows for run start, brainstorm, phase finalization, task assignment, worker start, proof pass/fail, audit result, parity result, rollback, quarantine, merge, model outcome, and completion.

Advanced runs also emit reasoning-state, reasoning-artifact, reasoning-lane, memory-capsule, heartbeat, and parity-gap events. The line budget remains 512 bytes so daemon status can tail logs cheaply.

Live proof runs may declare bounded `evidence_inputs`, `live_call_budget`, and `proofs`. When live calls are required, the runner fails closed on missing runtime/provider access, exhausted budget, or invalid JSON after retries; deterministic fake fallback is only allowed for non-live tests. Stage-0 proof output is generated from bounded evidence receipts and is written under `target/zyal/reasoning/<run_id>/stage0-master-plan.json`.

## Headless Operation

`jankurai-runner port-run` defaults to one tick. Use `--forever`, `--max-ticks`, `--tick-interval-secs`, and `--stop-file` for headless operation:

```bash
rtk jankurai-runner --repo . --run-id port-smoke port-run --config port.json --max-ticks 3 --tick-interval-secs 5
```

`jekko daemon start --port-run <config>` starts the port runner in forever mode unless `--max-ticks` is supplied. `jekko daemon status` reports the durable run id, phase, current reasoning artifact, active lane count, last event, parity gap count, memory capsule count, model reliability winner, and last Jankurai score when present.

## Artifacts

Advanced fake and live runs should produce:

```text
target/zyal/runs/<run_id>/events.jsonl
target/zyal/reasoning/<run_id>/reasoning-graph.json
target/zyal/reasoning/<run_id>/stage0-master-plan.json
target/zyal/reasoning/<run_id>/reasoning-benchmark.json
target/zyal/parity/<run_id>/generated_manifest.json
target/zyal/parity/<run_id>/approved-ci.txt
target/zyal/parity/<run_id>/raw.jsonl
target/zyal/parity/<run_id>/summary.json
target/zyal/parity/<run_id>/gaps.json
```

## Validation

Use the focused lane during development:

```bash
rtk just zyal-port-fast
```

Before handoff on broad port-runtime work, run the changed-path proof lanes and a Jankurai audit:

```bash
rtk git diff --check
rtk jankurai audit . --mode advisory --json .jankurai/repo-score.json --md .jankurai/repo-score.md
```

## Super-Agent Mode

`jekko port-run --super <manifest>` is the integration wrapper for the
Phase F+H super-agent kernel. It ties together the SuperWorkflow profile
(`zyalc`), the supervisor planner (`zyal-supervisor`), and the durable
phase store so an operator can drive a 9-12 phase super-workflow from a
single command.

### Flow

```text
.zyal manifest
    | (zyalc compile, Profile::SuperWorkflow)
    v
canonical superworkflow JSON
    | (zyal_supervisor::SuperWorkflow::from_json + validate_manifest)
    v
phase DAG (execution_layers / ready_phases)
    | (SupervisorStore::seed_run, 8 SQLite tables)
    v
durable supervisor state at --db path
    | (wave walker; STUB phase body today, --live follow-up)
    v
phase rows marked complete; events emitted to
target/zyal/runs/<run_id>/events.jsonl
```

### Flags

| Flag | Meaning |
|---|---|
| `--super <MANIFEST>` | Start a new run from a `.zyal` superworkflow manifest. |
| `--dry-run` | Print the wave plan JSON without touching the supervisor DB. |
| `--resume <RUN_ID>` | Reopen a run, reset in-flight `Running` phases to ready, and continue. Mutually exclusive with `--super`. |
| `--status <RUN_ID>` | Print persisted phase + task rows as JSON. Read-only. |
| `--db <PATH>` | Override the supervisor SQLite path (default `~/.jekko/zyal-supervisor.sqlite`). |
| `--max-stages <N>` | Cap completed stages; remaining phases land in `Blocked` with `stopped at max_stages`. |
| `--time-budget-hours <H>` | Stop after the wall-clock budget; remaining phases land in `Blocked` with `stopped at time_budget`. |

### Recipe

Compile + dry-run + execute the canonical 12-stage example:

```bash
# Compile the manifest (idempotent; --check verifies drift).
rtk zyalc compile agent/zyal/ambitious-superworkflow.zyal

# Inspect the wave plan without writing state.
rtk jekko port-run --super agent/zyal/ambitious-superworkflow.zyal --dry-run

# Run end-to-end, capped at the first 4 phases for a smoke.
rtk jekko port-run --super agent/zyal/ambitious-superworkflow.zyal --max-stages 4

# Tail events live in a second shell.
rtk jekko watch ambitious-superworkflow-template --format plain

# Show persisted state on demand.
rtk jekko port-run --status ambitious-superworkflow-template
```

The example manifest at `agent/zyal/ambitious-superworkflow.zyal` emits
the canonical 12-stage plan
(`source_of_truth -> architecture_blueprint -> repo_graph_bootstrap ->
contracts_and_slices -> parallel_subsystems -> integration_fusion ->
parity_lab -> parity_gap_closure -> performance_closure ->
hardening_security -> docs_release_ops -> final_signoff`) to
`agent/superworkflows/ambitious-superworkflow.superworkflow.json`. See
`docs/ZYAL/SUPER_REASONING_WORKFLOWS.md` for the live driver subsection
and per-stage contract.
