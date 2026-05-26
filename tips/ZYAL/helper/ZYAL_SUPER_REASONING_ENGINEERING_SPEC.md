# ZYAL Super-Reasoning Workflow Engineering Spec

## Summary

This spec defines a runtime-first extension path for making ZYAL a durable mission-control layer for very large, multi-day, multi-agent engineering and research efforts. The target class is not Redis-specific; Redis parity is only an example of the size and rigor expected. The design lets a swarm of weaker agents behave like a stronger system by forcing decomposition, independent reasoning, evidence capture, memory compounding, graph-aware edits, parity closure, performance closure, and final signoff.

The recommended implementation is a **typed Super Reasoning plan** attached to each daemon record and derived from existing ZYAL blocks (`workflow`, `fleet`, `memory`, `repo_intelligence`, `evidence`, `approvals`, `budgets`, `sandbox`, `observability`, `checkpoint`, `done`). This avoids adding a Redis-shaped workflow and avoids a premature ZYAL contract bump. The plan can later become a first-class generated schema block after runtime behavior proves out.

## Goals

1. Carry ambitious work to completion across many hours or days without losing phase state, evidence, or hard-won knowledge.
2. Allow 9-12 macro phases, each broken into bounded tasks, each task worked by redundant weak agents and reviewed by critics/reducers.
3. Permit safe parallel phase execution when phase dependencies allow it, then fuse, harden, and sign off.
4. Prevent sprawl with explicit phase budgets, write scopes, proof commands, artifact requirements, and promotion gates.
5. Compound knowledge through active memory stores that preserve decisions, evidence, failed hypotheses, parity gaps, performance gaps, and repo concepts.
6. Use a persistent sandbox and repo graph so future edits are faster, safer, and context-aware.
7. Make parity and performance closure first-class generic workflows, not Redis-specific workflows.
8. Keep raw private reasoning out of persistent memory; persist concise receipts, summaries, claims, evidence, and decisions.

## Non-goals

- Do not add Redis-specific commands, test names, or assumptions to ZYAL.
- Do not rely on raw model confidence or model-only completion claims.
- Do not require a graph database dependency on day one; SQLite-backed graph tables are acceptable initially, with Kuzu/Neo4j as optional stores later.
- Do not permit agent-written memory to become trusted without verifier evidence or reviewer signoff.

## Existing foundation

The current ZYAL contract already contains the right primitives: durable `workflow`, `memory`, `evidence`, `approvals`, `fleet`, `sandbox`, `security`, `observability`, `checkpoint`, `rollback`, `budgets`, and `done` blocks. The missing piece is a runtime control plane that binds those primitives into a long-running, validated phase DAG and keeps phase progress, evidence, graph state, and parity/performance gaps in daemon metadata and/or the backing store.

The proposed diff adds `crates/jekko-runtime/src/daemon/super_reasoning.rs` and attaches a validated `SuperReasoningPlan` to `DaemonRecord.metadata["super_reasoning"]`. The implementation is deliberately generic and can be generated from a ZYAL runbook preview or built programmatically by the host.

## Architecture

### Layer 1: ZYAL authoring surface

Authors describe the mission with existing ZYAL blocks:

- `job`: objective, constraints, and risks.
- `workflow`: macro phase state machine.
- `fleet`: worker count, worktree isolation, heartbeat, and scheduling.
- `fan_out` / `dispatch` / `experiments` / `incubator`: redundant idea generation, critique, tournament selection, and risky-task incubation.
- `memory`: active stores and promotion policies.
- `repo_intelligence`: repo atlas, symbol/call/test index policy.
- `evidence`: receipts required before phase promotion.
- `approvals`: reviewer/human signoff gates.
- `sandbox`: persistent workspace and ramdisk policy.
- `checkpoint` / `rollback`: verified commits and safe reversal.
- `done`: objective-level completion gates.

### Layer 2: Host-normalized Super Reasoning plan

During preview, the host normalizes the ZYAL runbook into a typed plan:

```text
SuperReasoningPlan
  mission_id
  objective
  swarm policy
  memory compounding policy
  repo graph policy
  persistent sandbox policy
  optional parity closure policy
  hardening policy
  signoff policy
  phases[]
```

The plan validates:

- phase count is within the requested 9-12 macro-phase envelope;
- phase IDs are unique;
- dependencies form a DAG;
- parallel waves are computable;
- phase workers do not exceed the fleet cap;
- parity closure has reference/candidate/oracle/manifest when enabled;
- every phase has tasks, lanes, acceptance gates, and signoff policy.

### Layer 3: Durable daemon metadata and store

The daemon registry stores the plan plus derived execution aids:

```json
{
  "super_reasoning": {
    "schema_version": "super_reasoning/v1",
    "plan": { ... },
    "topological_phase_ids": ["source_of_truth", "architecture_blueprint", ...],
    "parallel_waves": [["source_of_truth"], ["architecture_blueprint", "repo_graph_bootstrap"], ...],
    "ready_phase_ids": ["source_of_truth"]
  }
}
```

This is not the final persistence model. It is the minimum runtime bridge. The next store migration should split this into durable tables:

- `super_reasoning_runs`
- `super_reasoning_phases`
- `super_reasoning_tasks`
- `super_reasoning_artifacts`
- `super_reasoning_memory_receipts`
- `super_reasoning_parity_gaps`
- `super_reasoning_perf_gaps`
- `repo_graph_nodes`
- `repo_graph_edges`

### Layer 4: Execution controller

The controller repeatedly:

1. computes ready phases from completed phases;
2. allocates workers to ready phases according to budgets and dependency waves;
3. runs blind planner/scout lanes first;
4. runs implementer lanes only after enough plan evidence exists;
5. runs critic/reducer lanes before promotion;
6. records receipts and memory summaries;
7. runs proof commands and parity/performance checks;
8. blocks promotion on unresolved critical objections, missing evidence, or failed tests;
9. checkpoints verified changes;
10. updates graph and memory before the next wave.

## Canonical 12-stage macro plan

The patch’s default mega-project plan uses these generic stages:

1. **Source of truth**: read upstream/reference docs, APIs, behavior contracts, test suites, and compatibility notes. Produce a non-negotiable acceptance ledger.
2. **Architecture blueprint**: design module boundaries, storage model, concurrency model, protocol/API model, migration strategy, and risk register.
3. **Repo graph bootstrap**: index functions, symbols, tests, call edges, dataflow hints, ownership, and blast radius.
4. **Contracts and slices**: convert the blueprint into independently testable slices and task ledgers.
5. **Parallel subsystems**: implement independent slices in worktrees with redundant weak-agent workers and critic lanes.
6. **Integration fusion**: merge non-conflicting verified slices, resolve interface drift, and re-run graph/test proofs.
7. **Parity lab**: create differential/golden/metamorphic/property/fuzz parity harnesses for reference-vs-candidate behavior.
8. **Parity gap closure**: triage and close parity gaps until the manifest-defined threshold is met.
9. **Performance closure**: benchmark reference vs candidate, identify hot paths, and close performance gaps without breaking parity.
10. **Hardening and security**: fuzzing, fault injection, stress tests, race checks, security review, and recovery tests.
11. **Docs, release, and operations**: usage docs, migration notes, CI gates, release checklist, and operational runbooks.
12. **Final signoff**: aggregate evidence, run full proof lanes, confirm clean working tree, and require reviewer/human signoff when configured.

## Swarm reasoning policy

Weak agents become stronger when the controller enforces structure:

- **Blind scouts** prevent early convergence.
- **Independent implementers** produce diverse patches in isolated worktrees.
- **Critics** search for contradictions, missing tests, overbroad edits, and unsupported claims.
- **Reducers** synthesize the best verified fragments rather than averaging opinions.
- **Adversarial reviewers** preserve negative results and unresolved objections.
- **Quorum gates** require multiple lanes to independently agree on a promoted result.
- **Evidence gates** make model-only claims non-promotable.

Recommended default ratios:

- 60-70% implementation/scout lanes;
- 20-30% critic/reviewer lanes;
- 10% reducer/integration lanes;
- at least two redundant weak-agent attempts for high-risk tasks;
- no main-worktree writes before verified phase promotion.

## Active memory and knowledge compounding

Memory should store durable, searchable, provenance-rich receipts:

- source-of-truth decisions;
- architecture decisions and rejected alternatives;
- function/module graph concepts;
- task outcomes;
- failing tests and minimal repros;
- parity gaps;
- performance gaps;
- reviewer objections;
- fixes that worked;
- negative results that prevent repeated dead ends.

Memory should not store raw private chain-of-thought. Store compressed summaries, claims, evidence references, commands run, artifacts produced, and confidence calibrated by verification outcomes.

Promotion rule example:

```text
scratch memory -> run memory only after proof command passes
run memory -> project memory only after phase signoff
project memory -> global concept memory only after reuse across multiple phases or repos
```

## Repo graph policy

The first implementation can be SQLite-backed:

- `nodes(id, kind, path, symbol, span, hash, metadata)`
- `edges(src, dst, kind, confidence, evidence)`
- `test_edges(test_id, covered_node_id, evidence)`
- `change_impact(change_id, node_id, risk_score)`

Indexers should build:

- file/module/symbol/function nodes;
- call edges;
- import edges;
- test-to-code edges;
- ownership/area tags;
- changed-function hashes;
- failure-to-symbol links from test output.

The graph refresh policy should run on start, on phase entry, after checkpoint, and after high-blast-radius edits.

## Persistent sandbox and ramdisk policy

Large parity projects need both persistence and speed:

- persistent sandbox root: `.jekko/sandboxes/<mission-id>/`;
- worktree roots per phase/worker;
- retained build caches;
- artifact archive per phase;
- ramdisk for hot test data when supported;
- no secrets in sandbox logs;
- network allowlist during research and deny-by-default during implementation.

Parity tests should prefer in-memory execution, local loopback, tempdirs, or ramdisk paths. The policy is generic: reference and candidate commands are configurable, and the harness only compares declared behavior.

## Parity closure policy

Parity closure needs a durable gap ledger:

```text
gap id
source test/harness
input seed / corpus pointer
reference output
candidate output
classification
owner phase/task
status
fix commit
regression test path
performance impact
```

The controller should never mark parity done because “the model thinks it is close.” It is done when the configured oracle and manifest thresholds are satisfied and the evidence bundle is archived.

## Performance closure policy

Performance closure starts after parity reaches a stable baseline. The controller should:

1. record reference and candidate benchmarks;
2. classify gaps by subsystem;
3. map hot paths to repo graph nodes;
4. run microbenchmarks before broad optimization;
5. preserve parity tests during optimization;
6. promote only when both correctness and performance evidence pass.

## Anti-sprawl controls

Each phase must have:

- explicit dependencies;
- write scope;
- worker cap;
- max iterations;
- max diff lines;
- expected artifacts;
- acceptance gates;
- signoff mode;
- rollback policy.

Any phase exceeding budget is parked, incubated, or escalated rather than silently expanding scope.

## Implementation milestones

### Milestone 1: Runtime plan model

- Add `daemon::super_reasoning` typed model.
- Validate phase DAG, worker caps, and parity policy.
- Attach normalized plan to daemon metadata.
- Expose ready-phase computation.
- Add unit tests.

### Milestone 2: Store-backed state

- Add store migrations for runs, phases, tasks, artifacts, gaps, and graph nodes/edges.
- Replace metadata-only storage with store-backed records.
- Preserve metadata summary for UI preview.

### Milestone 3: Preview normalization

- Convert ZYAL `workflow` + related blocks into a `SuperReasoningPlan` during preview.
- Show phase DAG, parallel waves, budgets, evidence gates, memory stores, graph policy, and unsupported runtime features on the Run Card.

### Milestone 4: Controller loop

- Implement phase scheduler.
- Allocate worktree workers.
- Route scouts, implementers, critics, reducers, and reviewers.
- Save receipts after each worker turn.
- Block promotion on failed gates.

### Milestone 5: Repo graph

- Build SQLite graph indexer.
- Link tests/failures/changes to functions.
- Feed graph slices into worker context.
- Refresh after checkpoint.

### Milestone 6: Parity/performance lab

- Add generic differential harness runner.
- Add ramdisk/tempdir integration.
- Record parity and performance gaps.
- Drive closure loops from gap ledgers.

### Milestone 7: Final hardening and signoff

- Add final evidence-bundle aggregation.
- Require reviewer/human signoff as configured.
- Generate final report with phase receipts, memory summary, graph delta, parity closure, performance closure, and rollback proof.

## Acceptance tests

1. A valid default mega-project plan has 12 phases and validates.
2. A plan with fewer than 9 or more than 12 phases is rejected by default.
3. A cyclic phase graph is rejected.
4. Ready-phase computation returns only dependency-satisfied phases.
5. Parallel waves are deterministic and topologically valid.
6. A phase exceeding fleet worker cap is rejected.
7. An enabled parity policy without reference/candidate/oracle/manifest is rejected.
8. A registered daemon stores super-reasoning metadata and can rehydrate the plan.
9. Memory promotion refuses raw reasoning traces and accepts evidence receipts.
10. Parity closure cannot complete with open blocking gaps.
11. Performance closure cannot weaken parity gates.
12. Final signoff cannot pass without all phase receipts and required evidence.

## Operational recommendation

Adopt this in two steps. First, land the typed plan model and the generic mega-project example. Second, wire preview normalization and store-backed execution. This gives ZYAL an immediate, testable control-plane primitive while preserving the existing contract and avoiding over-engineering for any single target project.
