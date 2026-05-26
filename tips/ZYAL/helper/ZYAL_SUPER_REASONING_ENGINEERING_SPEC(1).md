# ZYAL Super Reasoning Engineering Spec

## Executive Summary

This spec proposes a generic **ZYAL Super Reasoning** runtime contract for very ambitious, long-running workflows such as “rewrite a mature system from scratch with 100% behavior parity.” Redis is only an example; the design applies to any target/replacement problem with discoverable contracts, tests, docs, benchmarks, and runtime behavior.

The core change is to make ZYAL’s existing powerful pieces operate as one disciplined long-horizon system:

- macro-plane planning with 9-12 clear stages;
- phase DAG execution with safe parallelism;
- many weaker agents working through bounded write scopes;
- graph-fed context instead of unbounded project sprawl;
- active memory with verified positive and negative knowledge;
- reducer/verifier signoff before phase completion;
- persistent worktrees/sandboxes for multi-day continuity;
- target-switched parity and performance closure;
- evidence bundles that can be audited after the run.

The accompanying `zyal_super_reasoning.diff` adds a new `super_reasoning` runtime policy module, wires it into `PortRuntimeOptions`, updates the port and advanced reasoning runners to produce and validate macro-stage plans, records the policy in daemon specs, and adds documentation plus a generic runbook.

## Current State

Jekko already has strong primitives:

1. ZYAL runbook/profile handling through `zyalc`.
2. Durable port workflow state in SQLite.
3. Advanced reasoning artifacts, edges, lanes, memory capsules, and model reliability.
4. A lightweight repository graph for files, Rust symbols, imports, calls, and tests.
5. Jankurai gates for audit/repair/regression discipline.
6. Hero/Judge and advanced reasoning flows for multi-lane prompt/reasoning work.
7. Parity lab artifacts: generated manifests, approved case lists, raw JSONL, summaries, and gap reports.
8. Example runbooks for generic ports, advanced reasoning, Redis/Jedis-style workflows, and live proof runs.

The gap is not “missing all the parts.” The gap is that the parts need a **single first-class contract** that forces ambitious runs to stay organized for hours or days.

## Goals

### Functional Goals

- Generate a generic 9-12 stage macro plan before broad implementation.
- Decompose each stage into phase tasks with explicit write scopes and proof lanes.
- Allow phase parallelism only when dependencies and write-scope disjointness permit it.
- Persist durable state and receipts for every phase/task/lane/gate.
- Give workers bounded context packs from graph slices and verified memory.
- Require reducer/verifier/Jankurai/parity signoff before phase completion.
- Maintain active memory across ticks: episodic, semantic, procedural, and negative.
- Close parity and performance gaps through target-switched evidence, not model claims.
- Preserve worktrees and caches between ticks while supporting garbage collection.
- Keep raw private reasoning out of durable storage.

### Non-Goals

- Do not hard-code Redis, SQLite, Postgres, or any target-specific stage list.
- Do not replace the existing port runner, advanced reasoning runner, parity lab, or Jankurai.
- Do not require a new external graph DB before the SQLite-backed graph path is useful.
- Do not let memory become a free-form scratchpad.
- Do not let workers mutate reference repositories.

## Proposed Runtime Contract

Add a `super_reasoning` block:

```yaml
super_reasoning:
  enabled: true
  macro_stage_target: 10
  parallel_phases:
    enabled: true
    max_parallel_phases: 3
    require_dependency_edges: true
    disjoint_write_scopes_required: true
  active_memory:
    episodic: true
    semantic: true
    procedural: true
    negative: true
    max_context_tokens: 24000
    write_requires:
      - verified_or_rejected_status
      - source_artifact_hash
      - verifier_or_reducer_approval
      - no_raw_chain_of_thought
  graph:
    store: sqlite
    incremental: true
    feed_workers: true
    slice_node_budget: 256
    include_tests: true
    include_callers: true
    include_callees: true
  parity:
    required_case_tags: [required, approved]
    gap_task_prefix: parity-gap
    require_perf_data: true
    prefer_ramdisk: true
    default_p95_ms_max_ratio: 1.25
  sandbox:
    run_root: ".zyal/runs/${run.id}"
    worktree_root: ".zyal/worktrees/${run.id}"
    keep_between_ticks: true
    gc_after: 14d
    read_only_reference_repos: true
  signoff:
    require_reducer: true
    require_verifier: true
    require_jankurai: true
    require_parity_delta: true
    max_unresolved_objections: 0
```

## Macro Plane

The runtime clamps `macro_stage_target` to 9-12. The default is 10.

| Stage | Purpose | Parallelization |
|---|---|---|
| Contract capture | Freeze objective, acceptance criteria, non-goals, source-use rules, and risk budget. | Serial root |
| Evidence map | Index docs/source/tests/examples/benchmarks/gaps. | After contract |
| Reference harness | Build target-switched harness and approved manifests before implementation claims. | After contract |
| Architecture skeleton | Establish candidate architecture, adapters, and scaffolding. | After evidence map |
| Semantic surface | Implement public APIs/protocol surfaces under parity tests. | After harness + skeleton |
| Core execution | Implement core algorithms/data flow. | After semantic surface |
| Durability and state | Persistence/recovery/state-machine semantics where relevant. | Can overlap with core only when scopes are disjoint |
| Integration fusion | Merge compatible branches and heal cross-phase regressions. | After core/durability |
| Parity expansion | Grow approved coverage and spawn gap tasks. | After fusion |
| Performance closure | Measure and close candidate/reference perf gaps. | After parity expansion |
| Adversarial hardening | Optional fuzz/red-team/chaos/security/resource passes. | After perf closure |
| Release signoff | Optional final evidence packet and acceptance report. | Terminal |

The macro-plane is intentionally generic. A Redis-like run would fill these stages with Redis-specific phase tasks only after evidence retrieval and reducer signoff.

## Runtime Flow

1. **Capture target request.** Store target, replacement, source-use constraints, acceptance criteria, non-goals, and risk budget.
2. **Retrieve evidence.** Build repo graph, ingest bounded evidence inputs, and capture reference docs/tests/examples.
3. **Brainstorm macro stages.** Run diverse blind planner lanes.
4. **Critique macro stages.** Find omissions, overlap, unsafe assumptions, target-specific hard-coding, and missing proof lanes.
5. **Reduce macro plan.** Produce the 9-12 stage plan and phase/task ownership.
6. **Validate macro plan.** Reject plans outside stage count, missing tasks, missing write scopes, or missing proof lanes.
7. **Phase planning.** For each stage, decompose into tasks with write scopes, graph slices, memory packs, and proof lanes.
8. **Swarm execution.** Workers operate in persistent worktrees with disjoint scopes.
9. **Phase verification.** Run proof lane, Jankurai, targeted tests, and parity delta.
10. **Phase signoff.** Reducer and verifier accept/reject claims. Write memory capsules.
11. **Fusion.** Merge verified branches onto integration, re-run system gates, rollback on red.
12. **Parity/perf closure.** Generate/approve cases, run reference/candidate adapters, spawn gap tasks, and repeat.
13. **Release signoff.** Produce final evidence bundle and acceptance packet.

## Swarm Roles

| Role | Responsibility |
|---|---|
| Supervisor | Owns run state, stop rules, budget, and phase DAG. |
| Framer | Turns the user objective into a task contract. |
| Retriever | Builds graph/memory/evidence packs. |
| Planner lanes | Propose macro and phase plans with strategy diversity. |
| Builder lanes | Implement bounded task slices. |
| Critic lanes | Falsify assumptions and identify missing proof. |
| Verifier lanes | Run executable/source-grounded checks. |
| Reducer | Merges lane outputs into a host-owned decision. |
| Memory curator | Writes verified/rejected capsules only. |
| Parity generator | Generates target-switched cases and manifests. |
| Perf closer | Analyzes and closes performance gaps. |
| Hard escalator | Uses power models for stuck, high-risk, or cross-cutting failures. |

## Active Memory

### Memory Families

- **Episodic:** run events, phase outcomes, failure histories.
- **Semantic:** stable facts about target/candidate behavior.
- **Procedural:** reusable verified procedures, harness patterns, debugging playbooks.
- **Negative:** rejected approaches, false hypotheses, flaky tests, perf traps.

### Write Gate

Permanent memory requires:

- `verified` or `rejected` status;
- source artifact id/hash;
- external-grounding or stronger evidence;
- reducer/verifier approval;
- no raw private reasoning;
- falsifying evidence for negative memory.

### Read Policy

Workers receive only a bounded memory pack:

- stage objective;
- task-specific past successes/failures;
- relevant semantic claims;
- relevant procedures;
- negative memory for similar failures;
- graph links and evidence hashes.

## Graph DB / Repo Intelligence

Start with the existing SQLite graph and extend toward a richer code graph.

### Node Types

- files, docs, tests;
- modules, functions, methods, structs, enums, impls;
- commands/protocol operations;
- phases, tasks, workers;
- parity cases, parity gaps, perf budgets;
- reasoning artifacts, memory capsules.

### Edge Types

- contains, imports, calls, tests;
- implements, derives_from;
- phase_owns, task_touches, changed_by;
- parity_covers, gap_from_case;
- supports, critiques, reduces, verifies;
- memory_from_artifact.

### Query API

Workers should ask for:

- graph slice by write scope;
- impacted tests for a function/file;
- callers/callees for touched symbols;
- parity cases covering a command/API;
- prior memory capsules related to a failure signature;
- phase/task dependency neighborhood.

## Persistent Sandbox

Each run gets:

```text
.zyal/runs/<run_id>/
.zyal/worktrees/<run_id>/<worker_id>/
target/zyal/runs/<run_id>/events.jsonl
target/zyal/reasoning/<run_id>/
target/zyal/parity/<run_id>/
```

Rules:

- Reference repos are read-only by default.
- Worker branches are namespaced.
- Dirty primary worktree blocks start unless explicitly allowed.
- Worktrees persist across ticks for build caches and context continuity.
- GC prunes stale unmerged worktrees after policy horizon.
- Capabilities and path write leases are enforced before shell/tools.

## Parity Lab

The parity lab should follow a Redline-style evidence model:

- generated manifest;
- approved CI case list;
- raw JSONL;
- summary JSON;
- gaps JSON;
- official-evidence JSON with hashes;
- report generation that verifies input hash binding.

### Gap Closure

Each failing/missing/skipped/perf-over-budget case becomes a task:

```text
parity-gap::<case_id>::correctness
parity-gap::<case_id>::missing-case
parity-gap::<case_id>::perf
```

Each gap task receives the case, reference/candidate outputs, graph slice, suspected modules, and required proof lane.

### RAMDisk / In-Memory Mode

For large parity suites:

- use tmpfs/RAM-disk for temp roots when available;
- prefer in-memory server/config modes where the target supports them;
- record environment and path in provenance;
- keep correctness determinism independent of memory mode.

## Data Model

The patch intentionally reuses existing tables where possible:

- `daemon_port_target`
- `daemon_port_phase`
- `daemon_port_task`
- `daemon_reasoning_artifact`
- `daemon_reasoning_edge`
- `daemon_reasoning_lane`
- `daemon_memory_capsule`
- `daemon_repo_graph_node`
- `daemon_repo_graph_edge`
- `daemon_parity_case`
- `daemon_parity_run`
- `daemon_parity_result`
- `daemon_perf_budget`
- `daemon_model_outcome`
- `daemon_model_reliability`

Future migrations can add:

- `daemon_phase_gate`
- `daemon_context_pack`
- `daemon_sandbox`
- `daemon_parity_gap_task`
- `daemon_memory_claim`
- `daemon_graph_query_cache`

## Patch Contents

The proposed diff adds:

1. `crates/jankurai-runner/src/super_reasoning.rs`
   - `SuperReasoningConfig`
   - `ParallelPhasePolicy`
   - `ActiveMemoryPolicy`
   - `GraphContextPolicy`
   - `SuperParityPolicy`
   - `PersistentSandboxPolicy`
   - `PhaseSignoffPolicy`
   - `draft_super_master_plan`
   - `validate_super_macro_plan`
   - `worker_context_contract`
   - `phase_memory_capsule`

2. `crates/jankurai-runner/src/lib.rs`
   - exports `super_reasoning`.

3. `crates/jankurai-runner/src/port.rs`
   - adds `PortRuntimeOptions.super_reasoning`;
   - adds `draft_master_plan_with_runtime`;
   - adds `validate_master_plan_with_runtime`;
   - keeps old five-stage starter plan when disabled.

4. `crates/jankurai-runner/src/port_runner.rs`
   - uses runtime-aware planning;
   - validates macro plans;
   - adds a focused super-plan persistence test.

5. `crates/jankurai-runner/src/reasoning_runner.rs`
   - uses super macro plan in fake advanced ticks;
   - validates live reducer plans when super mode is enabled;
   - marks run blocked on invalid macro plan.

6. `crates/jankurai-runner/src/daemon_store.rs`
   - records `super_reasoning` in daemon spec JSON.

7. Documentation and examples:
   - `docs/ZYAL/SUPER_REASONING.md`
   - `docs/ZYAL/PORT_WORKFLOW.md`
   - `docs/ZYAL/examples/34-super-port-foundry.zyal`
   - `docs/ZYAL/examples/README.md`

## Acceptance Criteria

### Unit

- `draft_super_master_plan` returns 9-12 stages.
- Each stage has at least one execution task and one signoff task.
- Each task has a non-empty write scope.
- Each task has a proof lane.
- `validate_super_macro_plan` rejects orphan stages.
- `phase_memory_capsule` is eligible for durable memory writes.
- `worker_context_contract` forbids raw reasoning and preserves scope.

### Runner

- Default port tick still produces the legacy five-stage plan.
- `super_reasoning.enabled=true` produces the macro-stage plan.
- Macro stages persist to `daemon_port_phase`.
- Phase tasks persist to `daemon_port_task`.
- Advanced fake ticks still write parity artifacts.
- Invalid live macro plans block the run with `master_plan_validation`.

### Integration

- `rtk just zyal-port-fast`
- `rtk cargo test -p jankurai-runner super_reasoning`
- `rtk cargo test -p jankurai-runner port_runner`
- `rtk cargo test -p jankurai-runner reasoning_runner`
- `rtk git diff --check`
- `rtk jankurai audit . --mode advisory --json .jankurai/repo-score.json --md .jankurai/repo-score.md`

### Documentation

- New example parses through `zyalc` daemon validation.
- Examples README count updates.
- Run Card should display `super_reasoning` summary in a follow-up UI patch.

## Rollout Plan

### Phase 1: Runtime Contract

Land the proposed patch. This gives a stable config shape, generic macro planner, validation, docs, and tests.

### Phase 2: Scheduler

Add phase-DAG scheduling:

- task dependency rows/edges;
- write-scope conflict detection;
- max parallel phase workers;
- integration branch lease;
- worker context-pack emission.

### Phase 3: Graph Context API

Add graph query helpers:

- `graph_slice_for_task`;
- `tests_for_write_scope`;
- `callers_callees_for_symbol`;
- `parity_cases_for_surface`;
- `memory_capsules_for_task`.

### Phase 4: Memory Kernel

Add durable memory claim APIs:

- positive/negative claim write gate;
- contradiction detection;
- memory decay/compaction;
- poison guard for untrusted/web content;
- cross-run retrieval.

### Phase 5: Parity Runner Hardening

Add Redline-style official evidence:

- hash-bound official evidence JSON;
- report verification;
- RAM-disk/temp-root controls;
- gap-task emission;
- perf regression tracking.

### Phase 6: TUI / Daemon UX

Expose super-mode status:

- current macro stage;
- active phase DAG;
- active workers;
- blocker/gap count;
- memory capsule count;
- last graph update;
- parity/perf gate status;
- run stop/resume controls.

## Failure Modes and Guards

| Failure | Guard |
|---|---|
| Sprawl | write scopes, phase DAG, reducer gates |
| Worker hallucination | executable evidence and confidence caps |
| Model agreement without proof | verifier/Jankurai/parity signoff |
| Context rot | graph slices + active memory + compaction |
| Poisoned memory | write gates, taint, source hashes, negative evidence |
| Reference mutation | read-only reference repo policy |
| Unbounded cost | live call budget, max workers, stop files |
| Perf ignored | perf budgets are parity failures |
| Fake completion | done forbids model-only claims and missing cases |
| Long-run drift | persistent receipts and SQLite run state |

## Example: Redis-Scale Rewrite Without Redis-Specific Engineering

For a request such as “rewrite Redis from the ground up in Rust with 100% parity,” the super flow should not start by coding. It should:

1. Capture Redis-like acceptance criteria and source-use rules.
2. Evidence-map docs, command specs, tests, benchmarks, and behavior examples.
3. Build a target-switched command/protocol harness.
4. Generate a 9-12 stage macro plan from evidence.
5. Split each phase into tasks with disjoint scopes.
6. Run workers in persistent worktrees.
7. Fuse verified branches.
8. Expand parity cases from command families and edge cases.
9. Close correctness gaps.
10. Close performance gaps.
11. Harden with fuzz/chaos/resource tests.
12. Produce a final evidence bundle.

The same flow works for any ambitious system replacement.
