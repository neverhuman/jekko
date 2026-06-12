# ZYAL — The Operating Contract for Autonomous Agents

> **Historical**: This document references the pre-Rust-port stack (Bun/OpenTUI/Solid/etc.). The current implementation is Rust-native (Ratatui/Crossterm). See [docs/archive/historical/](docs/archive/historical/) for the migration record.

> **Moonshot**: Make autonomous multi-hour, multi-agent software engineering sessions as safe, observable, and reproducible as a CI pipeline — then make them better than any human team.

ZYAL is the **host-enforced agent operating contract** embedded in Jekko. It is a strict, declarative YAML runbook that gives the *host* — never the model — total control over the lifecycle of unbounded agentic work. Every daemon run is bounded, observable, durable, evidence-gated, and subject to human approval at critical gates.

## Canonical Schema Reference

The canonical human-facing schema now lives in [`docs/ZYAL/SPEC.md`](docs/ZYAL/SPEC.md). This mission document stays as product narrative and operating context; it should not be treated as the source of truth for field definitions or compatibility rules.

## The Problem We're Solving

Today's AI coding assistants are **single-turn request machines**. You ask, it answers, you verify. This works for small tasks but collapses at the scale that matters: refactoring a monorepo, fixing 200 failing tests, implementing a feature across 50 files, auditing an entire codebase for security vulnerabilities, or running a continuous integration daemon that never sleeps.

The failure modes are predictable and devastating:

- **Unbounded autonomy** → the model rewrites half the codebase chasing a red herring
- **No memory across sessions** → it rediscovers the same dead ends every time
- **No evidence discipline** → "tests pass" is the only promotion criterion
- **No safety boundary** → `git push --force` is one hallucination away
- **No cost control** → a runaway loop burns $500 before anyone notices
- **Vibe coding** → deleting tests, weakening assertions, `@ts-ignore`, silent catches

ZYAL eliminates every one of these failure modes **structurally** — not by asking the model to remember rules, but by enforcing them in the runtime where the model cannot circumvent them.

## The Vision: Autonomous Engineering at Scale

ZYAL's endgame is not "a better copilot." It is the **operating system for autonomous software engineering**:

1. **Infinite agentic loops** — daemons that run for hours or days, checkpointing, rotating context, and committing verified changes while you sleep.
2. **Multi-agent orchestration** — supervisor agents dispatching work to parallel builder agents in isolated git worktrees, with a critic agent on a different provider catching correlated blind spots.
3. **Evidence-gated promotion** — no code reaches production without typed proof bundles: test results, affected files manifest, rollback plan, risk delta. Model self-claims are structurally rejected.
4. **Hypothesis tournaments** — competing implementation strategies racing in isolated branches, scored by weighted criteria, judged by a blind cross-provider critic. Failed lanes become negative memory so the system never repeats a dead end.
5. **Anti-vibe engineering** — structural gates that block test deletion, assertion weakening, silent catches, fake-data fallbacks, and `@ts-ignore`. Bug fixes require a failing test first.
6. **Cost sovereignty** — nested budgets at run, task, iteration, and experiment scope. `RUN_FOREVER` is honest: it's lease-bounded with renewal requiring human approval + progress evidence.
7. **Observable gold mode** — when ZYAL is active, the TUI transforms: gold theme overlay, `∞ ZYAL MODE` sidebar with live loop count, token usage, cost tracking, and daemon phase. Instant YAML validation badges in the prompt input (`✓ ZYAL` / `✗ ZYAL: <error>`).

---

## Architecture

### Core Principles

1. **Host owns the loop.** The model never decides when to start, stop, or promote. The host evaluates stop conditions, gates promotions with evidence, and enforces budgets.
2. **Preview before arming.** Every ZYAL block can be previewed without execution. The `ZYAL_ARM RUN_FOREVER` sentinel is a deliberate, separate human action.
3. **SQLite is truth.** All daemon state — runs, tasks, passes, iterations, events — lives in SQLite. The `.jekko/daemon/` mirror is generated output for human inspection.
4. **Bounded by design.** Every loop has a circuit breaker. Every incubator has a budget. Every pass has a write scope. There are no unbounded swarms.
5. **Safety is structural.** Guardrails, constraints, capabilities, and permissions are enforced by the runtime, not by asking the model to remember rules.

### What ZYAL Is Not

- Not a prompt trick or hidden chain-of-thought store
- Not an unbounded swarm launcher
- Not a model-owned completion signal
- Not a path for assistant output to start or stop the daemon

---

## Runtime and Preview Implementation

The ZYAL implementation spans **30+ modules** in the Jekko codebase. Core daemon execution, parsing, preview, durable state, checkpointing, stop checks, task routing, and TUI observability are shipped. Higher-risk v2.1/v2.2/v2.3/v2.4 blocks are strict parser/preview contracts until every corresponding runtime enforcement path is wired.

### Parser & Schema (`agent-script/`)

| Module | Lines | Purpose |
|--------|-------|---------|
| `schema.ts` | ~85 KB | Type definitions for all 40+ top-level blocks |
| `parser.ts` | ~72 KB | Strict YAML parser with sentinel validation, hash computation, semantic checks |
| `activation.ts` | ~1 KB | Real-time ZYAL detection with instant valid/invalid/preview classification |
| `parser.test.ts` | ~35 KB | Parser and example coverage, including docs runbooks |

### Daemon Engine (`session/`)

| Module | Purpose |
|--------|---------|
| `daemon.ts` | Core daemon loop: iteration, checkpoint, stop evaluation, context rotation, session forking |
| `daemon-store.ts` | SQLite-backed durable state: runs, iterations, events, tasks, passes, memories, workers, artifacts |
| `daemon.sql.ts` | 9 relational tables with indexes for daemon state |
| `daemon-incubator.ts` | 8-pass task maturation: scout → idea → strengthen → critic → synthesize → prototype → review → compress |
| `daemon-task-router.ts` | Risk/readiness scoring, lane routing (normal/incubator/blocked/archive) |
| `daemon-task-promote.ts` | Evidence-gated promotion with requirement checking and objection resolution |
| `daemon-task-memory.ts` | Per-task memory: context capsules, findings, objections, decisions |
| `daemon-pass.ts` | Pass lifecycle: context modes (blind/inherit/strengthen/critic/pool/promotion/ledger_only) |
| `daemon-checkpoint.ts` | Git checkpoint: verify → add → commit → push with configurable assertions |
| `daemon-checks.ts` | Shell check runner with timeout, exit code, stdout matching, JSON path evaluation |
| `daemon-constraints.ts` | Runtime invariant enforcement with baseline capture and violation policies |
| `daemon-guardrails.ts` | Input pattern blocking + output validation with retry/block/pause/abort actions |
| `daemon-on-handler.ts` | Signal-driven event handlers with counter tracking and action dispatch |
| `daemon-hooks.ts` | Lifecycle hook resolution: on_start, before/after_iteration, before/after_checkpoint, on_promote, on_exhaust, on_stop |
| `daemon-retry.ts` | Configurable backoff: none/linear/exponential with jitter |
| `daemon-context.ts` | Iteration prompt builder with daemon state summary |
| `daemon-event.ts` | Typed event system: run status transitions, phase tracking |
| `daemon-workers.ts` | Worker pool management with heartbeat and lease tracking |
| `daemon-fanout.ts` | Parallel map-reduce / scatter-gather execution |
| `daemon-reduce.ts` | Result reduction: merge_all, best_score, vote, custom_shell |
| `daemon-workflow.ts` | Durable state machine execution with typed transitions and approval gates |
| `daemon-memory.ts` | Governed memory stores with scope, retention, compression, redaction, provenance |
| `daemon-evidence.ts` | Typed proof bundles with SHA-256 signing and tamper detection |
| `daemon-approvals.ts` | Human approval gates with roles, timeouts, escalation chains, auto-approve conditions |
| `daemon-assertions.ts` | Structured output contracts with JSON schema validation |
| `daemon-skills.ts` | Skill discovery, quarantine, and governed promotion lifecycle |
| `daemon-sandbox.ts` | Execution isolation: Docker, nsjail, Firecracker, process-level sandboxing |
| `daemon-security.ts` | Secrets brokering, credential rotation, audit logging |
| `daemon-observability.ts` | Spans, metrics, cost tracking, budget enforcement, structured reporting |
| `daemon-mcp.ts` | MCP server profile management |

### TUI Integration

| Module | Purpose |
|--------|---------|
| `zyal-flash.ts` | Multi-source gold overlay activation with fleet metrics aggregation |
| `zyal-sidebar.tsx` | **∞ ZYAL MODE** sidebar: live loops, tokens, cost, uptime, phase, status |
| `daemon-banner.tsx` | In-session daemon status banner |
| `jekko-gold.json` | 44-property gold theme: noir backgrounds, gold/amber primaries, full syntax + diff + markdown coverage |
| Prompt validation | Instant `✓ ZYAL` / `✗ ZYAL: <error>` badge with gold/red border tinting |

### HTTP API

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/daemon` | GET | List all runs with aggregated `_stats` (tokens, cost, iterations) |
| `/daemon/:runID` | GET | Get run details with stats |
| `/daemon/:runID/events` | GET | Event stream |
| `/daemon/:runID/tasks` | GET | Task list |
| `/daemon/:runID/tasks/:taskID` | GET | Task details |
| `/daemon/:runID/tasks/:taskID/passes` | GET | Pass history |
| `/daemon/:runID/tasks/:taskID/memory` | GET | Task memory |
| `/daemon/:runID/pause` | POST | Pause |
| `/daemon/:runID/resume` | POST | Resume |
| `/daemon/:runID/abort` | POST | Abort |
| `/daemon/:runID/compact` | POST | Request compaction |
| `/daemon/:runID/rotate-session` | POST | Fork session for context rotation |
| `/daemon/:runID/tasks/:taskID/incubate` | POST | Route task to incubator |
| `/daemon/:runID/tasks/:taskID/promote` | POST | Manual promotion |
| `/daemon/:runID/tasks/:taskID/block` | POST | Block a task |
| `/daemon/:runID/tasks/:taskID/archive` | POST | Archive a task |
| `/daemon/:runID/incubator` | GET | Incubator state |
| `/daemon/preview` | POST | Parse and preview without executing |
| `/session/:sessionID/daemon/start` | POST | Start a daemon run |

---

## Complete Block Reference

### Sentinel Format

```
<<<ZYAL v1:daemon id=my-script>>>
version: v1
intent: daemon
confirm: RUN_FOREVER
# ... YAML body ...
<<<END_ZYAL id=my-script>>>
ZYAL_ARM RUN_FOREVER id=my-script
```

### All 40+ Top-Level Blocks

**v1 Core (15):** `version`, `intent`, `confirm`, `id`, `job`, `loop`, `stop`, `context`, `checkpoint`, `tasks`, `incubator`, `agents`, `mcp`, `permissions`, `ui`.

**v1.1 Safety (7):** `on`, `fan_out`, `guardrails`, `assertions`, `retry`, `hooks`, `constraints`.

**v2 Wave 1 — Evidence (4):** `workflow`, `memory`, `evidence`, `approvals`.

**v2 Wave 2 — Security (4):** `skills`, `sandbox`, `security`, `observability`.

**v2.1 Power Blocks (10):** `arming`, `capabilities`, `quality`, `experiments`, `models`, `budgets`, `triggers`, `rollback`, `done`, `repo_intelligence`.

**v2.2 Fleet (1):** `fleet` — single-session multi-worker orchestration with hard cap of 20 plus jnoccio-fusion telemetry integration.

**v2.3 Taint (1):** `taint` — origin-aware data-flow defence. Labels every inbound source (trusted_user, repo_file, tool_output, mcp_resource, web_content, assistant_generated) with a rank, forbids tainted content from triggering high-privilege actions (arm, approve, grant_capability, write_memory_procedural, exec_shell, install_skill, modify_objective, expose_secret) without human review or a signed sanitiser, and runs a configurable prompt-injection scanner on inbound bytes. Closes the prompt-injection / data-origin gap that no other block covers.

**v2.4 Research (1):** `research` — cited external evidence gathering with parallel fan-out, extraction, provenance receipts, and budgeted fallback behavior.

**Runtime coverage note:** v2.1/v2.2/v2.3/v2.4 blocks are strict parser and preview contracts today. The daemon runtime enforces core loop, stop checks, checkpointing, run state, incubator constraints, and fleet caps; blocks such as `arming` nonce/hash, `capabilities`, `quality`, `budgets`, `rollback`, `done`, `sandbox`, `security`, `taint`, and `research` are parsed, surfaced in the Run Card, and implemented in focused helper modules where present, but are not all wired into the shipped start loop yet. In this patch, `taint` remains parse/preview-only and `research` remains optional-backend and receipt-driven.

### Block Details

#### `job` — What & Why
```yaml
job:
  name: "Jankurai master loop"
  objective: |
    Run forever: audit, harvest work, execute tasks, commit verified changes.
  risk:
    - Long-running with autonomous git_commit
    - Touches arbitrary files
```

#### `loop` — Iteration Policy
```yaml
loop:
  policy: forever          # once | bounded | forever
  sleep: 5s
  continue_on: [assistant_stop, compaction]
  pause_on: [permission_denied]
  circuit_breaker:
    max_consecutive_errors: 5
    on_trip: pause          # pause | abort
```

#### `stop` — Host-Evaluated Termination
```yaml
stop:
  all:
    - shell: { command: "bun test", timeout: 60s, assert: { exit_code: 0 } }
    - git_clean: { allow_untracked: false }
  any:
    - shell: { command: "cat DONE", assert: { exit_code: 0 } }
```

#### `context` — Context Window Management
```yaml
context:
  strategy: hybrid         # soft | hard | hybrid
  compact_every: 10
  hard_clear_every: 20
  preserve: ["objective", "progress"]
```

#### `checkpoint` — Git Commit Gating
```yaml
checkpoint:
  when: after_verified_change
  noop_if_clean: true
  verify:
    - command: "bun test --timeout 30000"
      timeout: 60s
      assert: { exit_code: 0 }
  git: { add: ["."], push: ask }
```

#### `on` — Signal-Driven Event Handlers
Signals: `assistant_stop`, `max_steps`, `compaction`, `permission_denied`, `checkpoint_failed`, `no_progress`, `tool_calls_done`, `structured_output`, `error`, `cancelled`.
Actions: `switch_agent`, `run`, `incubate_current_task`, `checkpoint`, `pause`, `abort`, `notify`, `set_context`.

```yaml
on:
  - signal: no_progress
    count_gte: 3
    do: [{ switch_agent: plan }, { notify: "Stalled" }]
  - signal: checkpoint_failed
    do: [{ incubate_current_task: true }]
```

#### `guardrails` — Runtime Safety Validation
```yaml
guardrails:
  input:
    - name: no-force-push
      deny_patterns: ["git push --force", "git push -f"]
      action: block
  output:
    - name: type-safety
      shell: "npx tsgo --noEmit"
      on_fail: retry
      max_retries: 3
```

#### `constraints` — Runtime Invariants
```yaml
constraints:
  - name: test-count-stable
    check: { shell: "bun test --dry-run | grep -c test", timeout: 10s }
    baseline: capture_on_start
    invariant: gte_baseline      # gte_baseline | lte_baseline | equals_baseline | equals_zero | non_zero
    on_violation: pause
```

#### `incubator` — Hard Task Maturation

8 pass types: `scout` → `idea` → `strengthen` → `critic` → `synthesize` → `prototype` → `promotion_review` → `compress`.
Context modes: `blind`, `inherit`, `strengthen`, `critic`, `pool`, `promotion`, `ledger_only`.
Write scopes: `scratch_only`, `isolated_worktree`.

```yaml
incubator:
  enabled: true
  strategy: generate_pool_strengthen
  route_when:
    any: [{ repeated_attempts_gte: 3 }, { risk_score_gte: 0.7 }]
  budget: { max_passes_per_task: 7, max_rounds_per_task: 3 }
  readiness: { promote_at: 0.7, model_confidence_cap: 0.6 }
  promotion:
    require: [tests_identified, scope_bounded, plan_reviewed]
    block_on: { unresolved_critical_objections_gte: 1 }
```

#### `workflow` — Durable State Machine
```yaml
workflow:
  type: state_machine      # state_machine | dag | pipeline
  initial: discover
  states:
    discover:
      agent: plan
      writes: scratch_only
      produces: [impact_map]
      transitions: [{ to: plan, when: { evidence_exists: impact_map } }]
    plan:
      agent: plan
      requires: [impact_map]
      approval: plan_review
      transitions: [{ to: implement, when: { approval_granted: plan_review } }]
    implement:
      agent: build
      writes: isolated_worktree
      transitions:
        - { to: done, when: { all_checks_pass: true } }
        - { to: plan, when: { checks_failed: true } }
    done: { terminal: true }
  on_stuck: pause
  max_total_time: 24h
```

#### `evidence` — Typed Proof Bundles
```yaml
evidence:
  require_before_promote:
    - { type: test_results, must_pass: true }
    - { type: rollback_plan, must_exist: true }
    - { type: risk_delta, max_increase: 0.1 }
  sign: sha256
  archive: true
```

#### `approvals` — Human Decision Gates
```yaml
approvals:
  gates:
    plan_review:
      required_role: tech_lead
      timeout: 24h
      on_timeout: pause
      auto_approve_if: { risk_score_lt: 0.3, all_checks_pass: true }
  escalation: { chain: [tech_lead, staff_engineer], auto_escalate_after: 48h }
```

#### `arming` — Origin-Aware, Hash-Bound Arming
```yaml
arming:
  preview_hash_required: true
  host_nonce_required: true
  reject_inside_code_fence: true
  reject_from: [assistant_output, tool_output, web_content]
  accepted_origins: [trusted_user_message, signed_cli_input, host_ui_button]
  arm_token_single_use: true
  bound_to: [script_hash, user_id, repo, branch]
```

Current shipped behavior: this block is parsed and shown in preview. The start endpoint still accepts the simple trailing `ZYAL_ARM RUN_FOREVER id=<run-id>` sentinel; nonce/hash/single-use enforcement is a preview contract until the start API carries host-issued arm tokens.

#### `capabilities` — Least-Privilege Leases
```yaml
capabilities:
  default: deny
  rules:
    - { id: read-anywhere, tool: read, decision: allow }
    - { id: edit-src, tool: edit, paths: ["src/**"], decision: allow }
    - { id: shell-tests, tool: shell, command_regex: "^bun test", decision: allow, expires: 2h }
  command_floor:
    always_block: ["git push --force", "rm -rf /", "sudo ", "DROP DATABASE"]
```

#### `quality` — Anti-Vibe Engineering
```yaml
quality:
  anti_vibe:
    enabled: true
    fail_closed: true
    block_test_deletion: true
    block_assertion_weakening: true
    block_silent_catch: true
    block_fake_data_fallback: true
    block_ts_ignore: true
    require_failing_test_first_for_bugfix: true
  diff_budget: { max_files_changed: 25, max_added_lines: 1500, on_violation: require_approval }
```

#### `experiments` — Hypothesis Tournament
```yaml
experiments:
  strategy: disjoint_tournament
  fork_from: last_green_checkpoint
  diversity: { require_distinct_plan: true, min_plan_distance: 0.35 }
  lanes:
    - { id: minimal, hypothesis: "Smallest safe patch", isolation: git_worktree, timeout: 30m }
    - { id: tests-first, hypothesis: "Write failing tests, then fix", isolation: git_worktree }
    - { id: refactor, hypothesis: "Extract boundary, then port", isolation: git_worktree }
  scoring:
    weights: { tests_passed: 0.30, typecheck_passed: 0.20, diff_minimal: 0.10, security_clean: 0.20 }
    judge: { agent: critic, blind: true, must_use_different_provider: true }
  reduce: { strategy: best_verified_patch, require_final_verification: true }
  preserve_failed_lanes_as_negative_memory: true
```

#### `models` — Multi-Provider Routing
```yaml
models:
  profiles:
    planner: { provider: anthropic, model: claude-opus-4-7, reasoning: true }
    builder: { provider: anthropic, model: claude-sonnet-4-6 }
    critic: { provider: openai, model: gpt-5, reasoning: true }
  routes: { plan: planner, implement: builder, review: critic }
  critic: { must_use_different_provider: true }
  fallback: { on_rate_limit: summarizer, chain: [builder, summarizer], cooldown: 60s }
  confidence_cap: 0.6
```

#### `budgets` — Multi-Scope Cost Caps
```yaml
budgets:
  run: { wall_clock: 8h, cost_usd: 50.00, tokens: 5000000, on_exhaust: renew_with_approval }
  task: { iterations: 30, cost_usd: 5.00, on_exhaust: park }
  iteration: { tool_calls: 40, diff_lines: 2000, on_exhaust: pause }
  experiment_lane: { wall_clock: 30m, cost_usd: 2.00, on_exhaust: abort }
```

#### `triggers` — External Event Sources
```yaml
triggers:
  anti_recursion: true
  list:
    - { id: nightly, kind: cron, schedule: "0 4 * * *", allow_create_more_cron: false }
    - { id: ci-fail, kind: ci_failure, filter: { branch: main }, max_runs_per_sha: 1 }
    - { id: issue, kind: github_issue, filter: { label: zyal-ready } }
```

#### `rollback` — First-Class Reversibility
```yaml
rollback:
  required_when: { touches_paths: ["migration/**", "auth/**"], risk_score_gte: 0.6 }
  plan_required: true
  verify_command: "git apply --reverse --check < rollback.patch"
  on_failure_after_merge: revert_commit
```

#### `done` — Definition of Done (Host-Evaluated)
```yaml
done:
  require: [stop_conditions_met, evidence_complete, no_unresolved_critical_objections, rollback_plan_recorded]
  forbid: [model_only_claim, tests_not_run, pending_human_gate]
```

#### `repo_intelligence` — Codebase Awareness
```yaml
repo_intelligence:
  indexes: [symbols, dependency_graph, test_graph, ownership, generated_zones, security_sinks]
  scope_control: { require_scope_before_edit: true, max_initial_scope_files: 50 }
  blast_radius: { compute_on: [edit, checkpoint], pause_when_score_gte: 0.75 }
```

#### Additional Blocks: `sandbox`, `security`, `skills`, `observability`, `memory`, `agents`, `mcp`, `permissions`, `tasks`, `fan_out`, `assertions`, `retry`, `hooks`, `ui` — defined in `schema.ts`; enforcement ranges from shipped runtime engines to preview contracts as described in the runtime coverage note.

---

## Durable State & Mirror

All runtime state persists in **9 SQLite tables**: `daemon_run`, `daemon_iteration`, `daemon_event`, `daemon_task`, `daemon_task_pass`, `daemon_task_memory`, `daemon_worker`, `daemon_artifact`. The filesystem mirror at `.jekko/daemon/` is regenerated for human inspection:

```
.jekko/daemon/<runID>/
├── ledger.jsonl          # append-only event log
├── STATE.md              # human-readable run summary
└── tasks/<taskID>/
    ├── TASK.md            # task definition
    ├── CAPSULE.md         # context capsule
    ├── MEMORY.md          # accumulated memories
    ├── SCORE.json         # readiness scores
    ├── DECISIONS.jsonl    # pass decision log
    └── PASSES/
        ├── 001-scout.md
        ├── 002-idea.md
        └── 003-strengthen.md
```

---

## Safety Invariants

These are enforced **structurally by the runtime** where wired into the daemon loop; preview-only blocks are identified in the Run Card and the runtime coverage note above:

1. **Preview is separate from arming.** The `ZYAL_ARM` sentinel is a distinct human action.
2. **Runs are finite and host-checked.** Circuit breakers and stop conditions are runtime-enforced; v2.1 budget blocks are parsed/previewed and currently advisory in the start loop.
3. **SQLite is the source of truth.** All state is durable. Mirror files are regenerated.
4. **Prototype work writes only in isolated worktrees.** `writes: isolated_worktree` is enforced for prototype passes.
5. **Promotion requires host evidence.** A high `readiness_score` alone cannot promote — the host checks `require` evidence fields and `block_on` conditions.
6. **Model confidence is capped.** `model_confidence_cap` prevents the model from inflating its own readiness score.
7. **Guardrails and constraints are runtime-enforced.** Pattern matching and invariant checks happen in the host.
8. **On-handlers do not cascade.** A handler action cannot trigger another handler (max depth 1).
9. **Capabilities use deny-by-default + command floor.** These rules are parsed and previewed; full tool-gate wiring is pending.
10. **Anti-vibe gates are fail-closed.** These rules are parsed and previewed; full diff/promotion wiring is pending.

---

## TUI Experience

When ZYAL is active, the Jekko TUI transforms:

- **Gold theme overlay** — 44-property `jekko-gold` theme with noir backgrounds (#0d0b05), gold primaries (#ffd700), amber accents, and golden syntax highlighting
- **∞ ZYAL MODE sidebar** — live status, iteration count, total tokens, total cost, uptime, daemon phase
- **Daemon banner** — persistent status bar showing job name, iteration, phase, task, pass type, readiness score
- **Instant validation** — `✓ ZYAL` (gold) or `✗ ZYAL: <error>` (red) badge the moment you type or paste ZYAL syntax
- **Border tinting** — prompt border turns gold for valid ZYAL, red for invalid
- **Fleet metrics** — worker count, token breakdown (input/output/cache), tasks completed/incubated, cost USD

---

## Example Runbooks

The full set of flagship runbooks lives in [`docs/ZYAL/examples/`](ZYAL/examples/):

| File | Demonstrates |
|------|-------------|
| `01-fix-until-green.zyal` | Minimum viable daemon — anti-vibe, rollback, done definition |
| `02-hypothesis-tournament.zyal` | Competing hypotheses, blind cross-provider judging, negative memory |
| `03-billion-loc-monorepo.zyal` | Repo intelligence + scope control + blast radius |
| `04-fleet-portfolio.zyal` | Trigger-driven multi-issue dispatcher with budgets and anti-recursion |
| `05-secure-mcp-lockdown.zyal` | Capability leases + command floor + secrets brokering + sandbox |
| `06-evidence-graph-merge.zyal` | Proof lanes + merge witness + rollback verify |
| `07-self-improving-skills.zyal` | Skill quarantine → human review → promotion lifecycle |
| `08-full-power-runbook.zyal` | Every v2.1 power block in one runbook |

---

## Sandbox Loops: R&D Outside the Main Tree

A **sandbox loop** is a declarative ZYAL surface (Profile B, target=toml)
that lets agents execute experimental code in disposable workspaces with
permission allowlists, captured logs, and reviewable patch exports.

- Canonical spec: [`generated/sandbox-lanes.toml`](generated/sandbox-lanes.toml)
  (generated from `agent/sandbox-lanes.zyal` via `zyalc`).
- Runtime: [`crates/sandboxctl/`](crates/sandboxctl/) — three backends
  (`worktree` cross-platform, `bubblewrap` Linux-only, `docker`/`podman`
  cross-platform).
- User guide: [`docs/ZYAL/sandbox-loops.md`](docs/ZYAL/sandbox-loops.md).

Sandbox loops close the gap between "agent can execute commands" and
"agent's execution is constrained, auditable, and reversible". They embody
the **contract** principle of ZYAL: behaviour is declarative, isolation is
host-enforced, and every command lands in
`.agent/runs/<cmd_id>.{stdout,stderr,meta}` for review.

---

## The Endgame

ZYAL is not a configuration format. It is the **kernel of autonomous software engineering**.

The trajectory is clear: today, a single daemon loop fixes tests and commits changes while you sleep. Tomorrow, a fleet of ZYAL daemons — each with its own budget, capability lease, and evidence requirements — operates as a **self-improving engineering organization**: discovering work from issue trackers and CI failures, routing tasks by difficulty, incubating hard problems through multi-pass investigation, running hypothesis tournaments to find the best approach, promoting only when host-evaluated evidence is complete, and rolling back automatically when something goes wrong.

The model does the thinking. ZYAL does the governing. The human does the deciding.

That's the contract. That's the mission.
