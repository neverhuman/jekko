# ZYAL Super-Agent Workflow Kernel — Engineering Spec

**Date:** 2026-05-24  
**Target repo:** `neverhuman/jekko`  
**Patch artifact:** `zyal-super-agent-workflows.diff`

## 1. Executive summary

Jekko already has the right strategic pieces for extremely ambitious ZYAL workflows: durable SQLite-backed daemon stores, reasoning-artifact tables, memory-capsule tables, concept graphs, generic port/parity tables, repo-graph tables, sandbox/worktree examples, Jankurai gates, and flagship ZYAL examples for porting and advanced reasoning. The missing high-leverage layer is a runtime-wired **Super-Agent Workflow Kernel** that turns those declarative ZYAL blocks into a durable, bounded, inspectable execution plan before any weak agent starts doing work.

The proposed patch adds that layer without making Redis, SQLite, or any other target special. The kernel compiles generic `port`, `advanced_reasoning`, `repo_graph`, `parity_lab`, `model_policy`, `workflow`, and `done` blocks into a 9–12 stage master plan, seeds the existing daemon store tables, creates reasoning lanes and memory capsules, initializes repo-graph nodes, and provides a deterministic receipt for the run card and future HTTP/TUI surfaces.

The outcome is that a prompt like “rewrite Redis from the ground up in Rust with 100% parity” is not directly handed to a model. It is first converted into a structured operating plan with phase gates, isolated worktrees, active memory, parity/performance evidence, critic/reducer lanes, model reliability tracking, and anti-sprawl sign-off.

## 2. Current-state assessment

### 2.1 Strong foundations already present

Jekko is now a Rust workspace with separated core, runtime, store, provider, server, TUI, plugin, and automation boundaries. The architecture documentation keeps pure parsing/types in `jekko-core`, SQLite in `jekko-store`, daemon/tool orchestration in `jekko-runtime`, and HTTP/TUI presentation in their own crates. That is the correct dependency shape for a host-enforced ZYAL runtime.

The store layer already has the most important persistence surfaces:

- `daemon_reasoning_artifact`, `daemon_reasoning_edge`, `daemon_reasoning_lane`
- `daemon_memory_capsule`
- `daemon_model_reliability`
- `daemon_concept`, `daemon_concept_link`, `daemon_regression_cycle`
- `daemon_port_target`, `daemon_port_phase`, `daemon_port_task`
- `daemon_parity_case`, `daemon_parity_run`, `daemon_parity_result`, `daemon_perf_budget`
- `daemon_repo_graph_node`, `daemon_repo_graph_edge`
- `daemon_model_outcome`

The examples directory already contains advanced declarative intent, especially:

- `30-port-workflow-ultra.zyal` — generic replacement-port workflow
- `31-advanced-reasoning-foundry.zyal` — blind lanes, critics, reducers, verified memory
- `32-redis-jedis-port-foundry.zyal` — Redis-scale example without baking a Redis stage list into the YAML
- `33-redis-jedis-live-proof.zyal` — bounded live proof and deterministic benchmark sketch

The memory benchmark and AutoResearch work also proves that ZYAL can be used to govern long-running experiments, negative memory, trusted-core gates, and reducer-driven proof lanes.

### 2.2 Main gap

The Rust daemon registry is still a lifecycle scaffold. The store tables exist, and the examples express the desired behavior, but there is no single bridge that compiles the ZYAL runbook into durable phases, tasks, lanes, graph seeds, and memory artifacts before workers execute.

This gap matters because weak agents need a host-owned operating plan. Without it, they tend to sprawl, rediscover context, over-trust model confidence, and mistake partial tests for parity. The first runtime step must therefore be deterministic planning and durable state seeding.

## 3. Design goals

1. **Super reasoning from weak agents:** make weak agents powerful through host decomposition, parallel lanes, independent critics, reducers, executable proof, memory compounding, and reliability routing.
2. **No target-specific overengineering:** Redis is an example, not a schema. The same kernel must support any “reference system → replacement system” project.
3. **Macro plan before swarm:** every ambitious port gets a 9–12 stage plan before implementation begins.
4. **Stage-local tasking:** each stage gets bounded task seeds, write scopes, proof lanes, and sign-off rules.
5. **Parallel where safe:** stages and tasks can run in parallel only when dependencies and write scopes allow it.
6. **Fuse deliberately:** integration is a first-class phase with conflict healing, graph refresh, parity re-run, and critic sign-off.
7. **Active memory:** every artifact can become a verified positive memory, rejected negative memory, concept, concept link, or reliability datapoint.
8. **Smart graph substrate:** repo graph nodes and edges describe files, functions, modules, commands, tests, parity cases, stages, tasks, and relationships such as `calls`, `tests`, `covers`, `depends_on`, and `changed_by`.
9. **Parity/performance closure:** the endgame is not “tests pass”; it is approved parity cases, target-switched reference/candidate reports, perf budgets, and gap closure.
10. **Persistent sandbox:** workers operate in isolated worktrees or stronger sandboxes. The primary worktree is protected; reference repos are read-only.
11. **Evidence over confidence:** model confidence is capped unless executable evidence exists.
12. **Auditable stop/resume:** every phase, task, lane, graph seed, parity run, and memory write is durable and resumable.

## 4. Proposed architecture

### 4.1 Pure compiler in `jekko-core`

Add `crates/jekko-core/src/zyal_port_super.rs`.

Responsibilities:

- Extract the sentinel-wrapped ZYAL YAML body.
- Parse the existing advanced blocks:
  - `job`
  - `port`
  - `advanced_reasoning`
  - `repo_graph`
  - `parity_lab`
  - `model_policy`
  - `workflow`
  - `done`
- Validate high-value invariants before runtime starts:
  - `port.target` and `port.replacement` are present.
  - worker cap is 1–20.
  - raw reasoning storage is false for advanced reasoning.
  - parity lab has required fail-closed checks.
  - stage dependencies are acyclic.
- Compile a deterministic `CompiledPortSuperPlan` with:
  - stable `plan_hash`
  - 10-stage default plan
  - phase dependencies and parallelism hints
  - task seeds per phase
  - reasoning lane seeds
  - parity adapter plan
  - repo graph policy
  - memory policy
  - done/sign-off policy

The compiler is pure. It performs no I/O and does not call models. This respects the existing crate boundary.

### 4.2 Runtime seeding in `jekko-runtime`

Add `crates/jekko-runtime/src/super_agent.rs`.

Responsibilities:

- Accept a compiled plan and SQLite connection.
- Seed existing store tables:
  - `PortTargetRow`
  - `PortPhaseRow`
  - `PortTaskRow`
  - `ReasoningArtifactRow`
  - `ReasoningLaneRow`
  - `MemoryCapsuleRow`
  - `DaemonConceptRow`
  - `DaemonConceptLinkRow`
  - `RepoGraphNodeRow`
  - `RepoGraphEdgeRow`
- Return a `SuperAgentSeedReceipt` with counts and plan hash.
- Publishable future event: `daemon.super_agent.seeded`.

This keeps the host in charge: the run has a durable plan before workers start.

### 4.3 Existing store tables are enough for v1

No migration is required for the first runtime bridge. The patch intentionally uses the tables already present in `jekko-store`. This reduces risk and avoids turning the change into a schema-heavy redesign.

A later follow-up can add richer indexes or vector/FTS tables, but the first step should wire what is already present.

## 5. Default 10-stage master plan

For a generic reference-to-replacement port, the compiler emits these stages unless a future host-approved planner replaces them with an evidence-derived equivalent:

| # | Stage | Purpose | Parallelism |
|---:|---|---|---|
| 1 | Scope contract | Capture request, success criteria, non-goals, safety limits | Serial |
| 2 | Reference inventory | Map docs, commands/APIs, tests, fixtures, protocols, behavior sources | Serial after 1 |
| 3 | Repo graph and risk map | Build function/module/test/command graph and blast-radius map | Parallel with late stage 2 work |
| 4 | Architecture plan | Decide replacement architecture, module boundaries, persistence/runtime model | Serial after 2–3 |
| 5 | Core surface implementation | Implement central interfaces, command/API shell, basic runtime | Parallel with 7 after 4 |
| 6 | Feature semantics waves | Implement behavior clusters in bounded worktree tasks | Parallel waves after 5 |
| 7 | Parity harness and case library | Build reference/candidate adapters and approved parity cases | Parallel with 5–6 |
| 8 | Integration and fusion | Merge worktrees, resolve conflicts, refresh graph, run proof lanes | Serial after 5–7 |
| 9 | Performance closure | Benchmark, profile, close perf gaps, validate memory/RamDisk mode | Serial after 8 |
| 10 | Release sign-off | Final parity/perf/Jankurai/rollback/evidence gate | Serial after 9 |

This meets the requested 9–12 phase range while avoiding target-specific stage lists. For Redis-like projects, stage 6 naturally expands into command-family waves, but the kernel does not hard-code Redis commands.

## 6. Agent swarm model

The kernel should seed role lanes rather than launch an undifferentiated swarm:

| Role | Function | Memory behavior |
|---|---|---|
| Framer | Convert user goal into constraints, success criteria, non-goals | Writes scope memories only after reducer approval |
| Retriever | Pull repo graph, docs, tests, parity gaps, prior memories | Read-heavy; no permanent writes |
| Stage planner | Break a stage into bounded tasks and write scopes | Writes proposed task memories |
| Builder | Implements one bounded task in isolated worktree | Writes artifacts and test evidence |
| Parity author | Adds approved reference/candidate cases | Writes parity case proposals only |
| Critic | Finds flaws, missing evidence, unsafe merges, sprawl | Writes negative memory with falsifying evidence |
| Verifier | Runs proof commands and validates claims | Produces executable evidence artifacts |
| Reducer | Selects/fuses verified artifacts only | Produces phase summary and sign-off artifact |
| Performance hunter | Profiles and closes speed/memory gaps | Writes perf gap memories |
| Memory curator | Promotes/rejects memories and concepts | Only permanent memory writer |
| Integrator | Fuses branches and resolves conflicts | Writes integration receipts |

Model choice should be reliability-routed: outcomes are recorded per model/role/task kind, then future routing prefers models with high success/winner score and bounded cost.

## 7. Active memory and knowledge compounding

### 7.1 Memory write policy

The system should never store raw chain-of-thought. It stores compressed, evidence-bearing artifacts:

- claim summaries
- test/parity/perf receipts
- objections and their resolution status
- negative findings with falsifying evidence
- phase-level decisions
- graph deltas
- rollback notes

Permanent memory requires one of:

- verifier approval with executable evidence
- reducer approval from independent artifacts
- rejection with falsifying evidence
- human approval at a gate

### 7.2 Knowledge compounding loop

At the end of each phase:

1. Reducer summarizes verified facts.
2. Memory curator writes positive and negative capsules.
3. Concepts are upserted into `daemon_concept`.
4. Concept links record dependencies, contradictions, refinements, and supersessions.
5. Repo graph is refreshed.
6. Model reliability is updated.
7. Next phases retrieve the latest capsules, concepts, graph slices, and negative memory before planning.

This prevents weak agents from rediscovering dead ends or repeating disproven assumptions.

## 8. Repo graph requirements

The graph should start simple and practical:

### Node kinds

- `target`
- `replacement`
- `stage`
- `task`
- `file`
- `module`
- `function`
- `struct`
- `enum`
- `impl`
- `method`
- `command`
- `test`
- `parity_case`
- `perf_budget`
- `artifact`

### Edge kinds

- `contains`
- `imports`
- `calls`
- `tests`
- `implements`
- `depends_on`
- `phase_owns`
- `task_touches`
- `parity_covers`
- `changed_by`
- `supports`
- `critiques`
- `reduces`
- `regresses`
- `supersedes`

The first patch seeds the target, replacement, stage, task, and parity-plan nodes. A later graph indexer can add language-aware symbol extraction for Rust, TypeScript, Python, C, Go, and Java.

## 9. Persistent sandbox policy

Every worker gets:

- a unique worktree rooted under `.zyal/worktrees/<run_id>/<worker_or_task>`
- a declared write scope
- a capability lease
- a proof command set
- a branch name tied to task id
- a receipt directory under `target/zyal/runs/<run_id>/tasks/<task_id>/`

Reference repositories are read-only. Destructive operations on target repos are denied. Integration into the primary tree happens only through the integrator lane after proof gates pass.

For speed-sensitive parity suites, parity runners should support:

- in-memory process mode where possible
- RamDisk/tmpfs temp roots
- single-run reference/candidate harness invocation
- deterministic seeds
- hash-bound evidence bundles

## 10. Parity and performance harness

The parity lab should generalize the Redline-style idea:

- Reference adapter launches the original system.
- Candidate adapter launches the replacement.
- Each approved case runs against both and emits comparable observations.
- Raw results are JSONL.
- Summary is hash-bound to raw results and case manifest.
- Gaps are first-class tasks.
- Perf budgets compare p50/p95/p99 latency, throughput, RSS, CPU, and allocation counts where available.
- Required approved cases cannot be skipped.
- Missing perf data fails when a perf budget is declared.

For a Redis-scale port, parity case classes might include protocol framing, command semantics, persistence, expiry, transactions, pub/sub, Lua, streams, cluster behavior, ACLs, and compatibility edge cases. Those classes are generated from evidence, not baked into the kernel.

## 11. Phase sign-off contract

A phase cannot close unless all required gates pass:

- planned tasks completed or explicitly deferred with approval
- no critical unresolved objections
- proof commands pass
- Jankurai hard findings do not regress
- affected repo graph updated
- memory capsules verified or rejected
- rollback plan exists for risky changes
- parity/perf gaps either closed or converted into next-phase tasks
- worktrees fused or quarantined
- primary tree clean or checkpointed

## 12. Anti-sprawl controls

- Maximum worker cap: 20, default 10.
- Maximum active phase tasks: configurable; default 12.
- One task owns one write scope unless an integrator gate grants more.
- New files require graph node creation and task linkage.
- Test deletion, assertion weakening, silent catches, fake-data fallback, and unchecked ignores should be blocked by quality gates.
- Any phase with repeated failed attempts routes to incubator/critic rather than broadening scope.
- Long-running runs require budget renewal with progress evidence.

## 13. Implementation plan

### Patch 1 — included in `zyal-super-agent-workflows.diff`

- Add pure compiler: `jekko_core::zyal_port_super`.
- Add parser validation and deterministic default 10-stage plan.
- Add runtime seeder: `jekko_runtime::super_agent`.
- Seed existing store tables from compiled plan.
- Add docs and a generic runtime-wired example.
- Add narrow validation target.

### Patch 2 — recommended next

- Wire compile+seed into daemon start path.
- Add HTTP endpoints:
  - `GET /daemon/:runID/super-agent/plan`
  - `GET /daemon/:runID/super-agent/graph`
  - `GET /daemon/:runID/super-agent/memory`
  - `POST /daemon/:runID/super-agent/phase/:phaseID/signoff`
- Add TUI run-card section showing phase count, worker cap, raw-reasoning policy, parity gate, graph store, and memory policy.

### Patch 3 — recommended next

- Add parity runner crate or runtime module:
  - manifest parser
  - adapter launcher
  - RamDisk/tmpfs support
  - raw JSONL and hash-bound official evidence
  - gap-to-task generation

### Patch 4 — recommended next

- Add repo graph indexer:
  - Rust AST via `syn` or rustdoc JSON where available
  - fallback ripgrep/tree-sitter adapters
  - function/test relationship extraction
  - graph-delta receipts per task

### Patch 5 — recommended next

- Add model reliability routing into the worker selector.
- Use `daemon_model_reliability` and `daemon_model_outcome` to bias model/role assignment.
- Enforce confidence caps unless executable evidence exists.

## 14. Verification plan

Run after applying patch:

```sh
cargo test -p jekko-core --locked zyal_port_super
cargo test -p jekko-runtime --locked super_agent
cargo test -p jekko-store --locked daemon
cargo run -p zyalc -- compile --all --check
just fast
```

For integration once the start path is wired:

```sh
jekko daemon preview docs/ZYAL/examples/34-super-agent-port-kernel.zyal
# Review run card, then arm from trusted UI/CLI only.
```

For a parity harness follow-up:

```sh
zyal-parity run \
  --manifest tests/parity/<target>/manifest.json \
  --reference '<reference command>' \
  --candidate '<candidate command>' \
  --tmp-root ramdisk:auto \
  --raw target/zyal/parity/<run_id>/raw.jsonl \
  --summary target/zyal/parity/<run_id>/summary.json \
  --official-evidence target/zyal/parity/<run_id>/official-evidence.json
```

## 15. Redis-scale walk-through, kept generic

For “rewrite Redis in 100% Rust with 100% parity,” the runbook should declare:

- `port.target = Redis`
- `port.replacement = <candidate name>`
- reference repo path and candidate repo path
- read-only target repo policy
- parity lab adapters
- approved cases directory
- worker cap and sandbox policy
- Jankurai gate
- done criteria forbidding model-only claims and skipped required cases

Then the kernel emits the generic 10-stage plan. The reference inventory and parity author lanes discover actual behavior classes from source/docs/tests. The task system creates command-family work only after evidence supports it. Performance closure operates from measured gaps, not from guesses.

## 16. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Declarative examples outpace runtime | Compile+seed bridge makes preview/run-card/state durable before execution |
| Weak agents sprawl | Stage/task/write-scope gates and integrator-only fusion |
| Memory poisoning | Verified-or-rejected memory policy and no raw reasoning storage |
| Parity claims without evidence | Required approved cases, raw JSONL, summary hash, gap tasks |
| Performance ignored until too late | Dedicated phase 9 and perf budgets in parity lab |
| Repo graph too expensive | Seed coarse graph first; language-aware extraction follows incrementally |
| Overfitting to Redis | Use target/replacement abstraction and evidence-derived task classes |

## 17. Acceptance criteria

The kernel is successful when:

1. A generic port runbook compiles into a stable 9–12 stage plan.
2. The plan seeds durable target, phase, task, reasoning-lane, memory, concept, and repo-graph rows.
3. The receipt counts are deterministic.
4. The run card can show the plan before workers launch.
5. Workers can be started from seeded tasks with isolated write scopes.
6. Phase sign-off can be evaluated from evidence rows, parity reports, graph status, memory status, and Jankurai receipts.
7. The same mechanism works for Redis-scale projects, small library ports, feature-making, or ambitious research workflows.
