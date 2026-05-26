# ZYAL Super-Reasoning Swarm Engineering Spec

## Goal

Make ZYAL capable of carrying very ambitious, multi-day work to completion with a swarm of weaker agents: large rewrites, compatibility ports, deep research projects, parity/performance closure, and long-running improvement loops. The design is intentionally target-agnostic: Redis-in-Rust is only a stress case. The same machinery should work for databases, runtimes, protocol servers, compilers, libraries, and hard research/engineering problems.

## Current-state reading

Jekko is now a Rust-native, terminal-first coding agent with a Ratatui/Crossterm TUI, Axum HTTP/OpenAPI server, SQLite store, streaming provider runtime, and a Cargo workspace. ZYAL is the host-enforced runbook contract: the host owns loop control, evidence gates, budgets, checkpoints, memory, workers, and promotion decisions. The repository already contains the ingredients we need:

- **ZYAL contract surface**: workflow, memory, evidence, approvals, sandbox, observability, capabilities, budgets, fleet, taint, research, evidence graphs, and hero/judge prompt-evolution blocks.
- **Port workflow**: a generic replacement-port loop with target/replacement capture, repo graph, parity lab, model policy, workers, checkpointing, and done gates.
- **Advanced reasoning**: reasoning artifacts, evidence levels E0-E6, confidence caps, raw-reasoning redaction, reasoning lanes, memory capsules, and model reliability.
- **Parity lab**: target-switched reference/candidate adapters, RedlineDB-style manifests, raw JSONL, summary JSON, gaps JSON, and failure rules for zero/missing/skipped/failed/perfless/perf-over-budget cases.
- **Repo graph**: first-pass graph nodes/edges for files, docs, tests, Rust modules, functions, structs, enums, impls, methods, imports, calls, and test coverage.
- **Smartmemory/cogcore design corpus**: append-only ledger, FSRS-style resurfacing, Hebbian co-activation, A-MEM concept kernels, compounding/topic-hardening benchmark direction, and AutoResearch worktree loops.

The biggest gap is not a missing example. The strongest gap is turning the existing “generic port” runtime from a five-stage starter into a disciplined **12-stage super-reasoning kernel** with phase signoff, graph-fed workers, verified positive/negative memory, persistent sandbox/worktrees, parity/perf gap closure, and explicit anti-sprawl gates.

## Core proposal

Add **Super-Reasoning Port Swarm** as a first-class ZYAL workflow pattern:

1. **Derive a macro plan at runtime** from reference behavior, docs, tests, benchmarks, repo graph, and user constraints.
2. **Reduce to 9-12 signoff stages** before implementation begins. The patch sets the generic starter to 12 stages.
3. **Break each stage into dependency-aware tasks** with disjoint write scopes and proof lanes.
4. **Run independent workers in worktrees** where tasks are parallel-safe; otherwise sequence them behind graph dependencies.
5. **Persist all artifacts** in SQLite plus bounded NDJSON/filesystem mirrors.
6. **Use a smart graph database** as working memory for symbols/functions/tests/parity/phase ownership and changed-by relationships.
7. **Promote memory only after evidence**: verified positive memory and falsified negative memory are both permanent; raw chain-of-thought is never stored.
8. **Fuse lanes through reducers** that can combine only verified artifacts.
9. **Generate and run parity cases** against reference and candidate, preferably in memory/RamDisk when the target supports it.
10. **Close correctness and performance gaps** by converting every gap into a task until the parity gate passes.
11. **Block sprawl** through diff budgets, graph blast-radius checks, declared write scopes, test deletion/assertion weakening gates, and phase signoff.
12. **Finalize only with receipts**: complete phase signoffs, parity pass, perf budgets, Jankurai gate, rollback proof, durable memory capsules, and clean worktree.

## Canonical 12-stage macro plan

| # | Stage | Purpose | Parallelism |
|---:|---|---|---|
| 1 | `target_contract` | Capture request, target/replacement identity, reference safety boundaries, success metrics. | Sequential |
| 2 | `evidence_atlas` | Index docs/source/tests/benchmarks/examples and write provenance receipts. | Parallel retrieval/indexing |
| 3 | `behavior_inventory` | Enumerate observable behavior: APIs, protocols, commands, storage, errors, edge cases. | Parallel by subsystem |
| 4 | `macro_plan_tournament` | Run blind planners, critics, and reducer to choose stage plan. | Parallel blind lanes |
| 5 | `phase_dag` | Decompose stages into task DAG with disjoint write scopes and proof lanes. | Mostly parallel planning |
| 6 | `scaffold` | Create skeleton, adapters, parity harness, CI/proof plumbing. | Limited, because architecture coupling is high |
| 7 | `vertical_slices` | Build bounded behavior slices with failing/approved parity cases first. | Parallel when write scopes are disjoint |
| 8 | `integration_fusion` | Merge compatible lanes, heal conflicts, update graph and memory. | Sequential reducer plus targeted workers |
| 9 | `parity_expansion` | Generate approved parity cases until behavior map coverage is sufficient. | Parallel case generation/audit |
| 10 | `correctness_gap_closure` | Convert missing/skipped/failed parity cases into tasks and close them. | Parallel by independent gap cluster |
| 11 | `performance_gap_closure` | Convert perf over-budget gaps into tasks and close them without correctness regression. | Parallel by benchmark cluster |
| 12 | `release_readiness` | Final hardening, rollback, docs, audit, clean tree, acceptance packet. | Sequential signoff |

This gives the “9-12 clear stages” the user wants while remaining generic and non-Redis-specific.

## Runtime architecture

### Supervisor

The supervisor is not an all-powerful model. It is a host-owned orchestrator with these duties:

- Load ZYAL runbook and issue host-owned run ID.
- Build/update repo graph.
- Spawn bounded lanes/workers with write scopes.
- Enforce budget, taint, sandbox, capability, and evidence gates.
- Persist artifacts and events.
- Reduce candidates only from verified artifacts.
- Pause for approvals and budget renewal.

### Worker roles

- **Framer**: crystallizes target contract and success criteria.
- **Retriever/Atlas worker**: builds context packs from repo/docs/tests/reference behavior.
- **Planner**: proposes stages/tasks; blind lanes must not see each other’s candidates until reduction.
- **Builder**: implements scoped task slices in isolated worktrees.
- **Verifier**: runs tests, parity cases, graph checks, proof commands.
- **Critic**: tries to falsify plans, evidence, and gap closure claims.
- **Reducer**: combines verified artifacts and rejects unsupported claims.
- **Memory curator**: writes verified/rejected capsules, never raw reasoning.
- **Parity generator**: proposes target-switched cases, marks only reviewed cases as approved/required.
- **Performance auditor**: profiles candidate vs reference and creates perf gap tasks.

### Persistent sandbox/workspaces

- Primary repo remains the integration root.
- Worker roots live under `.zyal/worktrees/<run_id>/<worker_id>` or configured sandbox root.
- Reference target repositories are read-only by default.
- Worker write scopes are declared before spawn and checked against graph ownership.
- Generated artifacts go under ignored paths: `target/zyal/`, `.zyal/`, `.jankurai/`.
- Parity test execution should prefer in-memory mode or RamDisk/tmpfs when available. For database/server targets this means disabling persistence or pointing persistence to a temp memory-backed directory.

### Smart graph DB

The graph should eventually become the execution nervous system, not just an export:

**Node kinds**: files, modules, packages, functions, structs, enums, traits, impls, methods, tests, benchmarks, docs, commands, protocol messages, phases, tasks, parity cases, perf cases, memory capsules, model outcomes.

**Edge kinds**: contains, imports, calls, tests, implements, derives_from, parity_covers, phase_owns, changed_by, depends_on, reads, writes, benchmarks, supports, contradicts, reduces, verifies, regressed_by.

**Required graph operations**:

- `affected_tests(paths)` before edits/checkpoints.
- `parallel_safe(task_a, task_b)` based on write scopes and graph overlap.
- `blast_radius(diff)` with pause threshold.
- `parity_coverage(behavior_node)` to find missing tests.
- `gap_to_task(gap)` to bind parity/perf gaps to implementation scope.
- `memory_recall(query)` returning verified and rejected capsules with provenance.

The current lightweight graph builder already discovers Rust symbols and call/test edges; this spec’s runtime work upgrades it into a persisted queryable graph that feeds workers and gates merges.

## Memory and knowledge compounding

Memory must compound without poisoning the run:

- **Positive memory**: verified facts, winning design decisions, proof commands, parity coverage, performance lessons.
- **Negative memory**: failed approaches, falsified assumptions, rejected parity cases, perf regressions, reviewer objections.
- **Contradiction memory**: conflicting claims that require resolution before promotion.
- **Graph memory**: function/test/parity relationships observed during the run.

Rules:

1. Raw chain-of-thought is never stored.
2. Permanent memory requires `verified` or `rejected` status, grounded evidence, and nonempty provenance.
3. E0/E1/E2 claims remain confidence-capped and cannot promote code.
4. Negative memory requires falsifying evidence, not just “the model disliked it.”
5. Memory writes are hash-chained and signed/hashed for tamper evidence.
6. The memory curator can write summaries, not private reasoning transcripts.

This dovetails with smartmemory/cogcore’s append-only ledger and compounding direction.

## Parity and performance lab

The parity lab should become the default acceptance mechanism for replacement builds.

### Artifacts

- `target/zyal/parity/<run_id>/generated_manifest.json`
- `target/zyal/parity/<run_id>/approved-ci.txt`
- `target/zyal/parity/<run_id>/raw.jsonl`
- `target/zyal/parity/<run_id>/summary.json`
- `target/zyal/parity/<run_id>/gaps.json`

### Gate failures

A run cannot complete if any of these are true:

- Zero cases.
- Required/approved case missing.
- Required/approved case skipped.
- Any required case failed.
- Perf-required case missing perf data.
- Candidate/reference ratio exceeds case budget.
- Gaps exist in `gaps.json`.

### Gap closure loop

Every gap becomes a task with:

- gap category: missing_required, skipped_required, failed_case, missing_perf, perf_budget;
- owning phase and graph nodes;
- write scope;
- proof lane;
- rollback instruction;
- reviewer/critic signoff when broad or risky.

## Patch contents

The accompanying diff proposes these concrete changes:

1. `crates/jankurai-runner/src/port.rs`
   - Replace the five-stage starter plan with a generic 12-stage super-reasoning plan.
   - Add `DEFAULT_SUPER_REASONING_STAGE_COUNT`.
   - Update tests to assert stage count and first/last stages.

2. `crates/jankurai-runner/src/port_runner.rs`
   - Update fake tick test expectations for the new 12-stage plan.

3. `crates/jankurai-runner/src/reasoning.rs`
   - Add artifact kinds needed by macro planning, phase DAGs, function graphs, parity cases, perf gaps, signoff receipts, and contradiction logs.
   - Strengthen permanent-memory helper methods while preserving the current storage rules.

4. `docs/ZYAL/PORT_WORKFLOW.md`
   - Add the Super Reasoning / Ambitious Port Mode section and 12-stage macro plan.

5. `docs/ZYAL/SUPER_REASONING_WORKFLOWS.md`
   - Add a detailed runtime spec for super-reasoning swarms.

6. `docs/ZYAL/examples/34-super-reasoning-port-swarm.zyal`
   - Add a target-agnostic flagship runbook for ambitious replacement builds.

7. `docs/ZYAL/examples/README.md` and `docs/ZYAL/CHANGELOG.md`
   - Register the new example and document the change without a schema bump.

## Implementation roadmap

### Phase 0 — Land the docs/runbook/starter-plan patch

Acceptance:

- `cargo test -p jankurai-runner port::tests port_runner::tests reasoning::tests`
- `cargo run -p zyalc -- compile --all --check` once docs examples are included in discovery or existing parser lane covers them.
- Existing port-run fake tick now emits 12 stages and first fake task `task-target_contract`.

### Phase 1 — Convert runbook fields into runtime config

Acceptance:

- `jankurai-runner port-run --config` can accept stage strategy, sandbox, graph, parity, and memory options either from generated config or direct ZYAL extraction.
- Unknown super-reasoning fields are rejected or explicitly marked preview-only.
- The run status reports current macro stage and open gaps.

### Phase 2 — Graph DB upgrade

Acceptance:

- Persist graph nodes/edges for functions/methods/tests/parity/phase/task ownership.
- Add query helpers for affected tests, parallel-safety, blast radius, parity coverage, gap-to-task.
- Checkpoint pauses when graph export is stale or blast radius exceeds budget.

### Phase 3 — Worker scheduling and phase signoff

Acceptance:

- Tasks run in worktrees with disjoint scopes.
- Workers cannot modify reference target repos.
- Parallelism is disabled when graph overlap or dependency edges conflict.
- Each phase has signoff receipt before advancing.

### Phase 4 — Memory compounding

Acceptance:

- Verified/rejected capsules persist with provenance.
- Negative memory is retrieved in later planning prompts.
- Contradiction memory blocks reduction until resolved.
- No raw chain-of-thought appears in DB/artifacts.

### Phase 5 — Parity/perf gap closure

Acceptance:

- Required parity cases fail on missing/skipped/failed/perfless/perf-over-budget.
- `gaps.json` is converted into tasks.
- Performance closure cannot regress correctness.
- RamDisk/in-memory execution mode is supported where adapter declares it.

### Phase 6 — Long-running resume and observability

Acceptance:

- Crash/restart resumes from SQLite state.
- Worker leases expire and quarantine stuck workers.
- Daemon status shows current stage, active lanes, open parity/perf gaps, model reliability, memory capsule counts, and last Jankurai score.

### Phase 7 — Live proof and hard benchmark

Acceptance:

- Run bounded live proof with max model calls and fail-closed invalid JSON.
- Compare single-call baseline vs tournament/reducer on the same prompt.
- Show deterministic improvement metrics without storing raw reasoning.

## Test matrix

| Test | Purpose |
|---|---|
| Unit: port plan | 12 generic stages, no target-specific stage names, stable task IDs. |
| Unit: reasoning memory | Permanent writes require verified/rejected status and grounded evidence. |
| Unit: parity lab | zero/missing/skipped/failed/perfless/perf-over-budget fail gate. |
| Unit: graph | Rust functions/methods/calls/tests indexed and queryable. |
| Integration: fake port tick | DB rows, events, graph summary, 12-stage plan, fake worker pass. |
| Integration: advanced reasoning tick | confidence caps, raw reasoning redacted, memory capsules written only when eligible. |
| Integration: gap closure | parity gaps become tasks and close over repeated ticks. |
| Chaos: resume | kill/restart mid-phase, ensure no duplicate task commits and leases recover. |
| Security: reference read-only | destructive operation against target repo fails closed. |
| Performance: RamDisk parity | parity suite runs on tmpfs/RamDisk or in-memory adapter when supported. |

## Acceptance criteria for “Redis rewrite in Rust”-class jobs

A run of that class is not complete until all of this is true:

- Master plan generated from evidence, not baked target stages.
- All phases complete with signoff receipts.
- Graph DB knows changed functions, their callers/callees, affected tests, parity cases, and phase ownership.
- Reference repo remains read-only.
- Required parity suite passes with no skipped/missing/perfless cases.
- Performance budgets pass or are explicitly accepted by human final gate.
- Jankurai hard findings and cap regressions do not increase.
- Rollback plan is executable.
- Final worktree is clean.
- Memory capsules are verified/rejected and hash-chained.
- No raw chain-of-thought is stored.

## Risks and mitigations

- **Sprawl**: enforce graph-scoped write scopes, phase DAGs, diff budgets, and phase signoff.
- **False parity**: require approved/required cases and fail missing/skipped/perfless cases.
- **Poisoned memory**: allow permanent memory only after verifier/reducer approval or falsifying evidence.
- **Correlated model error**: use blind lanes and cross-provider critics when available.
- **Long-run drift**: persist state in SQLite, hash receipts, and require clean checkpoint gates.
- **Perf tunnel vision**: perf gap closure must re-run correctness parity before promotion.
- **Target-specific overfitting**: keep runbook generic; target behavior comes from evidence ingestion and parity cases, not hard-coded Redis phases.

## Bottom line

ZYAL is already pointed in the right direction. The highest-leverage next step is to make the generic port/reasoning runtime act like a disciplined engineering organization: 12-stage macro plan, graph-fed task routing, isolated worker execution, verified memory compounding, reducer-only fusion, and parity/performance gap closure as the hard definition of done.
