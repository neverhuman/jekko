# ZYAL Super-Reasoning Workflows

A SuperWorkflow is a 9-12 macro-phase plan that drives a multi-hour or
multi-day porting / hardening run end-to-end through a supervised DAG.
It is the next layer up from the per-run advanced-reasoning pipeline
documented in `PORT_WORKFLOW.md`: where a port-run owns a single
target/replacement pair, a SuperWorkflow owns the entire arc from
source-of-truth ingestion through final sign-off.

Authoring is declarative: a `.zyal` file with the
`target=superworkflow` pragma is compiled by `zyalc` to a canonical
JSON manifest, then executed by the `zyal-supervisor` planner under
`jekko port-run --super`.

## Profile pragma

```text
# zyal: declarative target=superworkflow schema=zyal/superworkflow@1
```

`zyalc` routes a file with this header through `Profile::SuperWorkflow`
which validates the DAG (no cycles, all `requires` references resolve)
and emits the canonical manifest JSON. The example at
`agent/zyal/ambitious-superworkflow.zyal` compiles to
`agent/superworkflows/ambitious-superworkflow.superworkflow.json`.

## Canonical 12-stage plan

The supervisor blueprint produced by the daemon canonical-phases
contract. Phase ids match the SuperWorkflow manifest one-to-one so a
runtime can register the manifest directly without remapping.

| # | Phase id | Purpose |
|---|---|---|
| 1 | `source_of_truth` | Capture the canonical target contract: docs, sources, test suite, runtime characteristics, perf budgets. |
| 2 | `architecture_blueprint` | Draft the replacement architecture; identify slices, ownership boundaries, parity gates. |
| 3 | `repo_graph_bootstrap` | Build the repo-intelligence graph: modules, dependencies, hot paths, taint zones. |
| 4 | `contracts_and_slices` | Lock the contracts: trait surfaces, RPC shapes, parity-test seed corpus. |
| 5 | `parallel_subsystems` | Fan out into disjoint worker lanes building each subsystem in isolation. |
| 6 | `integration_fusion` | Heal cross-phase integration until the replacement builds and runs as one system. |
| 7 | `parity_lab` | Drive the parity manifest target-switched against the reference; record raw runs + summary + gaps. |
| 8 | `parity_gap_closure` | Bounded follow-up tasks for each correctness gap until the parity gate is green. |
| 9 | `performance_closure` | Hit the perf budgets; close any p95/p99 ratio gaps. |
| 10 | `hardening_security` | Capability lockdown, sandbox audit, taint review, dependency hygiene. |
| 11 | `docs_release_ops` | Release notes, runbook updates, observability dashboards. |
| 12 | `final_signoff` | Reviewer + reducer approval, durable memory archive, run marked complete. |

Each phase declares its own gates, `requires`, and per-phase memory /
parallelism limits. The supervisor planner walks ready phases in
parallel up to the manifest's `max_parallel_phases`.

## Live driver

`jekko port-run --super <manifest>` is the operator entry point.

```text
.zyal manifest
    | zyalc compile (Profile::SuperWorkflow)
    v
canonical superworkflow JSON
    | zyal_supervisor::SuperWorkflow::from_json + validate_manifest
    v
phase DAG (execution_layers, ready_phases)
    | SupervisorStore::seed_run (8 SQLite tables, idempotent schema)
    v
durable state at --db <path>
    | wave walker; phase body is STUB today, --live follow-up
    v
phase rows transition Ready -> Running -> Complete;
events stream to target/zyal/runs/<run_id>/events.jsonl
```

### Flags

See `PORT_WORKFLOW.md#super-agent-mode` for the full table. Highlights:

- `--dry-run` prints the wave plan JSON without touching the DB.
- `--resume <RUN_ID>` reopens a run and resets in-flight `Running`
  phases to `Ready` before continuing.
- `--status <RUN_ID>` prints persisted phase + task rows as JSON
  without mutating state.
- `--max-stages <N>` and `--time-budget-hours <H>` are hard caps;
  remaining phases land in `Blocked` with a `stopped at ...` summary.

### Observability

Pair the driver with `jekko watch <run_id>` (see
`docs/ZYAL/OBSERVABILITY.md`) for a notify-based live tail. The
supervisor emits the standard ZYAL event vocabulary plus the watcher's
five new variants: `WorkerStall`, `WorkerQuarantine`, `WatcherStarted`,
`RemediationTriggered`, `JankuraiRegression`.

### Smoke recipe

```bash
rtk zyalc compile agent/zyal/ambitious-superworkflow.zyal
rtk jekko port-run --super agent/zyal/ambitious-superworkflow.zyal --dry-run
rtk jekko port-run --super agent/zyal/ambitious-superworkflow.zyal --max-stages 2
rtk jekko port-run --status ambitious-superworkflow-template
```

The CI-safe `just zyal-super-redis` recipe gates the live variant
behind `JEKKO_ZYAL_LIVE=1`; the deterministic dry-run path is always
safe to invoke.
