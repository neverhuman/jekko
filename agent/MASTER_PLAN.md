# Master Plan

This file is the canonical phase log referenced by `AGENTS.md`. It tracks the in-flight macro plan for the repo and the rolling status of each phase.

## Phase index

For explicit MASTER_PLAN / phase-mode work, the local phase workflow lives under `tips/phases/`. See `tips/phases/00-phase-index.md` for the per-phase reading order. Plans the user supplies in-conversation (paper, release, implementation, or handoff plans) are the controlling plan and bypass this index — see `CLAUDE.md` for the routing rule.

## Active macro plan: foamy-koala

Plan file: `/home/ubuntu/.claude/plans/please-study-this-the-foamy-koala.md`

Centralization + super-agent kernel work toward driving long-running multi-stage workloads (mega-runs) through a unified ZYAL backbone with multi-user credential routing, parallel tool-enabled reasoning, semantic memory, and live observability.

### Phase log

| Phase | Subject | Status | Commit(s) |
|---|---|---|---|
| A | Unblock both build trees (EventKind variant + jnoccio-fusion field scaffolding) | done | 5378ce11e |
| B1 | Scaffold zyal-core leaf crate (6 modules) | done | 8b1c045ac |
| B2.1 | Migrate CredentialSourcePolicy → zyal-core | done | d7d61ece0 |
| B2.2 | Migrate forbidden-content patterns → zyal-core (split: shape vs credential) | done | cb2a9de71 |
| B2.3 | Alias SuperReasoningArtifactContract → zyal_core::ArtifactContract | done | 0c8a9c5f5 |
| B2.4 | Alias ReasoningArtifactKind → zyal_core::ArtifactKind (+8 super-agent variants available) | done | 6a4bdeead |
| B2.5 | Split MemoryCapsule write-gate into named helpers | done | d06c90d35 |
| B2.6 | LaneId/RunId/ArtifactRef newtypes | scaffolded in B1; first usage lands in Phase F | — |
| C1 | Scaffold zyal-key-pool crate (pool / balancer / budget) | done | b288ca619 |
| C2 | Wire zyal-key-pool into jnoccio-fusion + jankurai-runner (UsersPool fanout + PolicyHook gate) | done | 983771fee |
| C3 | Create this MASTER_PLAN.md (closes `missing-agent-readable-docs` cap) | done | f081adf28 |
| D1 | ToolMode { Off, ReadOnly, Full } policy + per-role mapping | done | 830c1ee15 |
| D2 | Lift hardcoded JEKKO_RUN_DISABLE_TOOLS, add JEKKO_RUN_TOOL_ALLOWLIST | done | 830c1ee15 (same commit as D1) |
| D3-D5 | Parallel brainstorm via `futures::future::join_all` + reducer fence + tests | done | 2fcbe83a3 |
| E1 | Structured memory + promotion lifecycle (memory_kind, promotion_status, claim_text, approved_by_role) | done | 45d494672 |
| E2 substrate | embedding column + encode_embedding / decode_embedding / cosine_similarity | done | faf8b4cff |
| E2 runtime | OpenAICompatibleEmbedder + retrieve_for_run + jnoccio-fusion /v1/embeddings | done | fdc834bed |
| F1 | jekko-runtime daemon: SuperReasoningPlan registration API + canonical_phases() 12-stage builder | done | fb0adf07cf |
| F2 | jankurai-runner: SuperReasoningConfig extension (parallel/memory/graph/parity policies + draft_super_master_plan) | done | ff2237baf |
| F3 | 12-stage blueprint wiring | done via F2's canonical_stage_templates() (mirrors F1's canonical_phases names) | (covered by F2) |
| F4 | New zyal-supervisor crate (model + planner + SQLite store, 8 tables) | done | 73603a979 |
| F5 | zyalc Profile::SuperWorkflow + ambitious-superworkflow.zyal + emitted JSON | done | c5c597272 |
| G1 | Watcher metrics + remediation engine + 5 new EventKind variants | done | 63b7f43c4 |
| G2 | Ratatui dashboard (real frames + `--tui-once-snapshot` for CI testability) | done | folded into ac25749b7 (H live commit swept G2 files) |
| G3 | jekko-cli `watch` subcommand + notify-based tail loop | done | 71ed88380 (new files) + 4189941aa (CLI wiring) |
| G4 | jnoccio-fusion /metrics Prometheus endpoint (text/plain; version=0.0.4) | done | a89fa9927 |
| H scaffold | jekko port-run --super integration wrapper (compile→seed→plan→walk waves) | done | 94151ef80 |
| H live | --live flag + per-phase `jekko run` subprocess + JEKKO_ZYAL_LIVE gate + --per-phase-timeout-secs + --max-stages / --time-budget-hours enforcement | done | ac25749b7 |
| docs | PORT_WORKFLOW Super-Agent + OBSERVABILITY + MULTI_USER_KEYS + SUPER_REASONING_WORKFLOWS + CHANGELOG + examples/README | done | d4b0c0525 |
| review | code-reviewer pass over 22 commits — WEAK PASS | done (recommendations folded in) | — |
| fix | Cycle-detector dedupe across F1+F4+F5 (BTreeSet pre-dedupe of `depends_on`) | done | 838017b2c |
| fix | Cap regressions — stub→scaffold rename + explicit error capture + `_generated` JSON header in zyalc emit | done | (this branch) |
| fix | port_run.rs explicit-match cleanup + `<<<ZYAL …>>>` sentinel hoist | done | (this branch) |

### Heavy live MiniRedis run (deferred to a focused interactive session)

Everything needed to drive the heavy live workload is shipped. The next session can run:

```
JEKKO_ZYAL_LIVE=1 JEKKO_KEY_SOURCE_POLICY=users-only \
  cargo run -p jekko-cli --offline -- port-run \
    --super agent/zyal/ambitious-superworkflow.zyal \
    --live --time-budget-hours 4
# in another terminal:
cargo run -p jekko-cli --offline -- watch <run_id> --format tui
# and:
curl -s localhost:4317/metrics | grep fusion_model_requests_total
```

Phase H scaffold + H live + G2 + G3 + G4 + C2 give the operator everything the
plan promised: compile→seed→execute with multi-user balancing, live phase
subprocess invocation, Ratatui observability, and Prometheus scrape. The
session-cap on this plan is the time + spend budget for that single live run,
which is a separate decision.

### Naming conventions

- **`jankurai`** is the external code auditor binary (https://github.com/neverhuman/jankurai/). Pre-existing `crates/jankurai*` keep their names; NEW crates introduced by this plan use `zyal-*` (for backbone types) or `jekko-*` (for general infra).
- **`jekko`** is this repo / workspace.
- **`ZYAL`** is the orchestration / runbook system inside jekko.

### Pre-existing repo caps

Tracked but NOT introduced by this plan; address as part of relevant later phases or a dedicated cleanup pass:

- `release-readiness-gap` — needs release artifact docs/staging
- `no-agent-friendly-exception-pattern` — repo-wide pattern refactor
- `missing-agent-readable-docs` — partially addressed by this file; full closure when more agent-readable docs land
- `ci-local-parity` — `just ci-local` is missing CI steps the GitHub workflows run

The pre-commit hook at `tools/jankurai-hooks/pre-commit` is informational only — it computes pass/fail but does not exit non-zero on score regressions. Tightening it is a follow-up.
