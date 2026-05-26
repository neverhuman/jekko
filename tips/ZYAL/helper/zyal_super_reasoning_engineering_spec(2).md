# ZYAL Super Reasoning Workflows — Engineering Spec

## Goal

Add a first-class ZYAL workflow substrate for extremely ambitious, long-running challenges where many weaker agents must compound knowledge, coordinate safely, and converge all the way to a verified result. The target class is not one domain such as Redis; it is any project that needs macro planning, parallel work, durable memory, reference-vs-target evaluation, performance hardening, and final sign-off.

Examples of target classes:

- Greenfield parity rewrites of complex systems.
- Multi-month-scale refactors compressed into an autonomous swarm run.
- Open-ended scientific or mathematical research programs where hypotheses, evidence, contradictions, and negative results must survive compaction.
- Large compatibility ports where the true finish line is “reference behavior plus performance plus operational quality,” not merely “it builds.”

## Current-state reading

Jekko already has unusually strong primitives for this direction:

- Rust-native CLI/TUI/server/runtime/store/provider architecture.
- `zyalc` with strict source-profile detection and generated-zone support.
- `sandboxctl` and declarative sandbox lanes for isolated worktrees and artifact export.
- `jekko-store` SQLite persistence with migration discipline and in-memory support.
- `cogcore`, which already models append-only memory, concept/index state, feedback, contradiction pressure, forgetting, and deterministic rebuild.
- `agent-search`, which already separates search/provider transport from provenance and deduped evidence records.
- Advanced memory and QBank/benchmark assets that can be reused as gates for long-horizon memory quality.

The missing piece is a contract that turns these primitives into a durable “ambitious challenge supervisor.” Existing ZYAL has many useful blocks, but huge projects need a single high-level DAG that binds phases, agents, memory, evidence, repo graph, parity testing, performance loops, and sign-off into one runtime object.

## Proposed patch summary

The companion patch introduces:

1. **ZYAL Profile D: `target=superworkflow`**  
   A source-level ZYAL profile that compiles an ambitious workflow YAML body into a canonical JSON manifest.

2. **`zyal-supervisor` crate**  
   A minimal runtime substrate that validates manifests, computes phase readiness from dependency DAGs, persists run/phase/task/memory/evidence/sign-off state in SQLite, and exposes a CLI for host/runtime integration.

3. **Generic ambitious workflow template**  
   `agent/zyal/ambitious-superworkflow.zyal` captures an 11-phase macro plan suitable for parity rewrites, large ports, or serious research workflows without encoding Redis-specific behavior.

4. **Engineering documentation**  
   A new `docs/ZYAL/SUPER_REASONING_WORKFLOWS.md` describes the architecture, memory model, swarm coordination, phase gates, parity harness pattern, and rollout plan.

## Design principles

### 1. Make the macro plan explicit before coding

The supervisor requires 9–12 macro phases. This constraint intentionally forces decomposition before implementation starts. A good ambitious run should include:

1. Problem framing and invariants.
2. Reference and acceptance modeling.
3. Architecture and code/repo graph indexing.
4. Foundation runtime vertical slice.
5. Feature/task ledger generation.
6. Parallel implementation.
7. Differential/parity harness.
8. Gap closure loop.
9. Performance stabilization.
10. Security/reliability/docs hardening.
11. Release, archive, and long-tail task capture.

Nine phases are enough for simpler projects; twelve leave room for domain-specific splits. Fewer phases hides risk; more phases tends to create sprawl.

### 2. Split planning, execution, and sign-off

Weak agents become powerful only when roles are separated:

- **Planner agents** create phase/task decomposition.
- **Reference mappers** enumerate expected behavior.
- **Implementers** make scoped changes in isolated worktrees.
- **Reviewers/critics** challenge assumptions and catch sprawl.
- **Parity judges** own reference-vs-target diffs.
- **Performance leads** own profile and regression evidence.
- **Memory curators** distill lessons, failures, and decisions.
- **Graph indexers** maintain code-symbol/test/dependency graph state.
- **Release captains** fuse, prove, archive, and publish residual gaps.

The supervisor should not allow a phase to close just because an implementer says it is done. Closure requires artifacts, gates, and sign-off.

### 3. Treat memory as an active runtime, not a transcript

Every long run needs several memory layers:

| Layer | Purpose | Persistence |
|---|---|---|
| Run WAL | Immutable action/decision/evidence history | JSONL/SQLite append-only |
| Working memory | Current phase plan, owners, blockers | SQLite phase/task tables |
| Knowledge memory | Durable lessons, invariants, negative results, design decisions | SQLite + cogcore-backed recall |
| Evidence memory | External sources, benchmark reports, parity evidence, citations | provenance tables / JSONL |
| Repo graph memory | Symbols, files, tests, call/import edges, hotspots | SQLite graph tables |

Memory must be injectable at phase start and task assignment time. It should not require the entire raw transcript to fit into context.

### 4. Make negative memory first-class

For huge projects, failed attempts are valuable. The system should preserve:

- Rejected designs and the reason they failed.
- Parity gaps that were misdiagnosed.
- Performance fixes that hurt correctness.
- Tests that exposed flawed assumptions.
- Source evidence that contradicted the current plan.

Negative memory prevents a swarm of weaker agents from repeatedly rediscovering the same dead ends.

### 5. Use a repo graph to reduce future edit cost

The proposed `zyal_super_repo_symbols` and `zyal_super_repo_edges` tables are intentionally simple. They can be populated by tree-sitter, LSP, SCIP, rustdoc JSON, cargo metadata, language-specific analyzers, or direct grep fallbacks.

Minimum useful graph nodes:

- Files.
- Functions/methods/types.
- Public APIs.
- Tests and fixtures.
- Benchmarks.
- Protocol/command handlers.
- Performance hotspots.

Minimum useful graph edges:

- `contains` file → symbol.
- `calls` symbol → symbol.
- `imports` file → module/file.
- `tests` test → symbol/API.
- `covers` fixture/corpus → behavior/API.
- `regresses` benchmark → symbol/hotspot.

The graph allows future agents to ask, “What tests cover this behavior?”, “What code is near this parity gap?”, and “What should I not touch without retesting?”

### 6. Keep parallelism bounded by dependencies and merge risk

Parallelism is powerful but dangerous. The supervisor should enforce:

- Phase DAG dependencies.
- Task-level leases.
- Worktree isolation.
- Maximum touched-file budgets.
- Maximum parallel phases and tasks.
- Required merge reports.
- Graph-aware conflict detection.
- No unmerged high-value worktree cleanup without archival.

A swarm should not be a pile of unsynchronized patches.

### 7. Promote parity and performance to first-class gates

For compatibility projects, tests are not enough. The workflow needs an explicit reference-vs-target harness:

- Reference adapter.
- Candidate adapter.
- Corpus manifest.
- Deterministic evidence JSONL.
- Failure clustering.
- Regression promotion for fixed gaps.
- In-memory or ramdisk execution mode.
- Report generator tied to evidence rather than opinion.

The same pattern also works outside Redis-like systems: reference implementation vs new implementation, baseline proof vs new theorem attempt, old behavior vs refactored behavior, or simulation oracle vs optimized engine.

### 8. Preserve human-compatible sign-off

The supervisor should write artifacts a human can inspect:

- Problem framing.
- Phase DAG.
- Architecture proposal.
- Feature/task ledger.
- Merge report.
- Parity report.
- Performance report.
- Security/reliability report.
- Final proof report.
- Known gaps.
- Memory archive.

The goal is autonomous progress, not opaque autonomy.

## Runtime model

### Manifest lifecycle

1. Author `agent/zyal/<name>.zyal` with `target=superworkflow`.
2. `zyalc compile` validates phase shape and emits `agent/superworkflows/<name>.superworkflow.json`.
3. `zyal-supervisor validate` performs runtime-grade validation.
4. `zyal-supervisor init-run` creates a persistent run row and phase rows in SQLite.
5. Host runtime asks `zyal-supervisor ready` for executable phases.
6. Agents claim tasks inside ready phases.
7. Agents write evidence, memory, repo graph updates, and patches.
8. Phase gates run.
9. Reviewers sign off.
10. Supervisor marks phase complete and unlocks dependent phases.
11. Final phase archives memory and remaining known gaps.

### Store schema

The patch adds these SQLite tables:

- `zyal_super_runs`
- `zyal_super_phases`
- `zyal_super_tasks`
- `zyal_super_memory`
- `zyal_super_evidence`
- `zyal_super_repo_symbols`
- `zyal_super_repo_edges`
- `zyal_super_signoffs`

This is intentionally small. It can later be integrated into `jekko-store` migrations once the API stabilizes.

### Phase state machine

Allowed states:

- `pending`
- `ready`
- `running`
- `blocked`
- `review`
- `complete`
- `parked`
- `failed`

Recommended transitions:

```text
pending -> ready -> running -> review -> complete
pending -> blocked -> ready
running -> blocked
running -> failed
running -> parked
review -> running
review -> complete
```

The host should reject `complete` unless required artifacts and gates pass.

## Phase template for ambitious parity/rewrite work

The proposed default has 11 phases:

1. `P00_frame_ambition`
2. `P01_reference_and_acceptance`
3. `P02_architecture_and_graph_index`
4. `P03_foundation_runtime`
5. `P04_feature_ledger`
6. `P05_parallel_implementation`
7. `P06_parity_harness`
8. `P07_gap_closure`
9. `P08_performance_stabilization`
10. `P09_security_reliability_docs`
11. `P10_release_and_long_tail`

For a Redis-like challenge, these phases would map naturally to command/API inventory, protocol semantics, persistence behavior, replication/cluster semantics, differential command corpus, latency/throughput benchmarks, and final known-gap closure. But those details belong in task ledgers and adapters, not the generic ZYAL contract.

## Parity harness pattern

The Redline-style pattern should be generalized:

```text
case -> reference adapter -> normalized result
case -> candidate adapter -> normalized result
normalized diff -> evidence JSONL -> report -> gap ledger -> regression test
```

Key requirements:

- Deterministic corpus manifest.
- Explicit reference and candidate adapters.
- Per-case raw evidence.
- Normalized diff schema.
- Clustered gap ledger.
- Promotion of fixed gaps into regression tests.
- In-memory or ramdisk temp roots for high-volume test execution.
- Separate correctness and performance reports.

## Memory injection policy

At phase start, inject:

- Objective and invariants.
- Phase-specific acceptance criteria.
- Recently fixed gaps.
- Recently failed approaches.
- Graph hotspots for touched areas.
- Relevant prior sign-offs.
- Current parity/performance/security deltas.

At task assignment, inject only:

- Task scope.
- Related symbols/tests from repo graph.
- Relevant memory snippets.
- Required artifacts/gates.
- Known blockers and negative memories.

This keeps weaker agents focused and prevents context bloat.

## Anti-sprawl controls

The system should enforce or at least record:

- Scope contract per phase and task.
- Touched-file budget.
- No primary-checkout edits by default.
- Worktree lease ownership.
- Required patch export.
- Merge report and conflict summary.
- Review quorum for phase completion.
- Automatic parking of unresolved long-tail tasks.
- No final “done” without known-gap report.

## Rollout plan

### Milestone 1 — Compile and validate

- Add Profile D to `zyalc`.
- Add manifest validation.
- Add example template and generated zone.
- Add documentation.

### Milestone 2 — Durable run state

- Land `zyal-supervisor` store and CLI.
- Add phase readiness and sign-off commands.
- Add smoke tests around manifest validation and SQLite state.

### Milestone 3 — Host integration

- Wire the runtime to create runs from SuperWorkflow manifests.
- Expose ready phases/tasks to the swarm controller.
- Route memory/evidence writes into the store.

### Milestone 4 — Repo graph and parity harness

- Add graph indexer adapters.
- Add reference-vs-target parity runner contract.
- Add report schemas and corpus manifests.

### Milestone 5 — Full autonomous hardening loop

- Add failure clustering.
- Add performance regression loop.
- Add release/archive gates.
- Add final proof bundle generation.

## Acceptance criteria

The implementation is good enough when it can run a large generic challenge and produce:

- A phase DAG with 9–12 phases.
- Persistent run and phase state across restarts.
- Parallel worktree task execution without scope sprawl.
- Durable memory of decisions, evidence, failures, and sign-offs.
- Repo graph entries that help future edits.
- Reference-vs-target parity evidence.
- Gap closure reports.
- Performance reports.
- Final proof bundle and known-gap archive.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| The swarm creates incoherent patches | Worktree isolation, task leases, graph-aware merge reports, review quorum |
| Agents lose context after compaction | Phase summaries, memory injection, run WAL, negative memory |
| False “done” from shallow tests | Parity harness, evidence JSONL, gap ledger, final proof bundle |
| Over-engineering one domain | Keep domain specifics in adapters/task ledgers, not the SuperWorkflow schema |
| Graph database slows the run | Start with SQLite graph tables; allow optional Kuzu/Neo4j adapters later |
| Weak agents repeat mistakes | Negative memory and failure-cluster injection |
| Performance ignored until too late | Dedicated performance phase plus parity-preserving perf gates |
| Long-tail gaps disappear | Required known-gap archive and parked task ledger |

## Why this is the right next layer

Jekko already has source compilation, sandboxing, persistence, memory primitives, evidence provenance, and benchmark assets. A SuperWorkflow layer turns those into an operating system for ambitious reasoning: explicit plans, durable state, active memory, parallel work with bounded risk, and proof-driven convergence.
