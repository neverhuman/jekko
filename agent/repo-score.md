# jankurai Repo Score

- Standard: `jankurai`
- Auditor: `1.6.1`
- Schema: `1.9.0`
- Paper edition: `2026.05-ed8`
- Target stack ID: `rust-ts-vite-react-postgres-bounded-python`
- Target stack: `Rust core + TypeScript/React/Vite + PostgreSQL + generated contracts + exception-only Python AI/data service`
- Repo: `.`
- Run ID: `1781828332`
- Started at: `1781828332`
- Elapsed: `17402` ms
- Scope: `full`
- Raw score: `89`
- Final score: `89`
- Decision: `advisory`
- Minimum score: `85`
- Caps applied: `none`

## Hard Rule Caps

| Rule | Max Score | Applied |
| --- | ---: | --- |
| `no-root-agent-instructions` | 75 | no |
| `no-one-command-setup-or-validation` | 70 | no |
| `no-deterministic-fast-lane` | 65 | no |
| `no-security-lane-on-high-risk-repo` | 60 | no |
| `generated-contracts-or-public-api-drift-untested` | 80 | no |
| `python-direct-product-truth-or-db-ownership` | 72 | no |
| `no-secret-or-dependency-scanning-in-ci` | 78 | no |
| `no-jankurai-audit-lane-in-ci` | 82 | no |
| `jankurai-required-tool-ci-evidence-gap` | 88 | no |
| `non-optimal-product-language-found` | 74 | no |
| `too-much-python-in-product-surface` | 72 | no |
| `boundary-reclassification-evidence-gap` | 72 | no |
| `vibe-placeholders-in-product-code` | 68 | no |
| `fallback-soup-in-product-code` | 70 | no |
| `future-hostile-dead-language-in-product-code` | 64 | no |
| `severe-duplication-in-product-code` | 70 | no |
| `generated-zone-mutation-risk` | 76 | no |
| `direct-db-access-from-wrong-layer` | 66 | no |
| `missing-web-e2e-lane` | 82 | no |
| `missing-rendered-ux-qa-lane` | 84 | no |
| `prompt-injection-risk` | 78 | no |
| `overbroad-agent-agency` | 65 | no |
| `secret-like-content-detected` | 60 | no |
| `false-green-test-risk` | 76 | no |
| `destructive-migration-risk` | 70 | no |
| `authz-or-data-isolation-gap` | 78 | no |
| `input-boundary-gap` | 78 | no |
| `agent-tool-supply-chain-gap` | 78 | no |
| `release-readiness-gap` | 80 | no |
| `missing-rust-property-or-integration-tests` | 82 | no |
| `no-agent-friendly-exception-pattern` | 76 | no |
| `missing-agent-readable-docs` | 80 | no |
| `streaming-runtime-drift` | 78 | no |
| `rust-bad-behavior` | 72 | no |
| `sql-bad-behavior` | 72 | no |
| `typescript-bad-behavior` | 72 | no |
| `docker-bad-behavior` | 72 | no |
| `python-bad-behavior` | 72 | no |
| `ci-bad-behavior` | 70 | no |
| `git-bad-behavior` | 70 | no |
| `gittools-bad-behavior` | 70 | no |
| `release-bad-behavior` | 70 | no |
| `web-security-bad-behavior` | 68 | no |
| `repo-rot-bad-behavior` | 88 | no |
| `comment-hygiene-dangerous-residue` | 72 | no |
| `ci-local-parity` | 70 | no |

## Copy-Code Redundancy

- Status: `review` hard=`0` warning=`95` files=`1001`
- Policy: min-lines=`10` min-tokens=`100` max-findings=`50` include-tests=`false` strict=`false`
- Duplicate volume: lines=`290` tokens=`794` bytes=`7648`

- Notes:
  - hard classes are limited to exact active-source file matches and substantial exact same-name units
  - warning classes include same-body different-name units and token/block duplication
  - tests, fixtures, stories, config, Docker, and migrations are omitted unless --include-tests is set
  - showing the top 50 classes and omitting 45 lower-ranked classes

| Kind | Severity | Language | Lines | Tokens | Instances | Reason |
| --- | --- | --- | ---: | ---: | --- | --- |
| `ExactUnitDifferentName` | `Warning` | `rust` | 24 | 86 | `crates/jekko-runtime/src/file.rs:140-164, crates/jekko-runtime/src/permission.rs:298-322` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 8 | `crates/jekko-runtime/src/tool/edit/mod.rs:62-66, crates/jekko-runtime/src/tool/read.rs:55-59, crates/jekko-runtime/src/tool/task.rs:52-56, crates/jekko-runtime/src/tool/webfetch.rs:94-98, crates/jekko-runtime/src/tool/websearch.rs:83-87, crates/jekko-runtime/src/tool/write.rs:48-52` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 17 | 54 | `crates/memory-benchmark/src/corpus/real_papers/model.rs:219-236, crates/qbank-builder/src/core_types.rs:80-97` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 1 | `crates/memory-benchmark/src/candidates/arena/lane_08.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_09.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_10.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_11.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_12.rs:17-19, crates/memory-benchmark/src/candidates/arena/lane_13.rs:17-19, crates/memory-benchmark/src/candidates/arena/lane_14.rs:17-19, crates/memory-benchmark/src/candidates/arena/lane_15.rs:17-19` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 1 | `crates/memory-benchmark/src/candidates/arena/lane_04.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_05.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_06.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_07.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_16.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_17.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_18.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_19.rs:16-18` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 12 | `crates/xtask/src/close_issues.rs:127-131, crates/xtask/src/compliance_close.rs:183-187, crates/xtask/src/pr_compliance.rs:146-150, crates/xtask/src/pr_standards.rs:143-147` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 18 | `crates/xtask/src/publish_release.rs:91-97, crates/xtask/src/publish_release_package.rs:183-189, crates/xtask/src/publish_release_registry.rs:229-235` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 12 | 33 | `crates/jekko-store/build.rs:197-209, crates/jekko-store/src/migration.rs:237-249` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 12 | 14 | `crates/jekko-cli/src/cmd/zyal_dispatch.rs:104-116, crates/jekko-cli/src/cmd/zyal_run.rs:955-967` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 10 | 31 | `crates/xtask/src/publish_npm_package.rs:44-54, crates/xtask/src/publish_release_package.rs:142-152` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 3 | 14 | `crates/xtask/src/commands/security_lane.rs:189-192, crates/xtask/src/commands/security_lane.rs:223-226, crates/xtask/src/commands/security_lane.rs:245-248, crates/xtask/src/commands/security_lane.rs:273-276` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 9 | 15 | `crates/jekko-runner/src/bin_main/hero_series/files.rs:25-34, crates/jekko-runner/src/hero_judge_runner_helpers.rs:133-142` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 1 | 4 | `crates/jekko-runtime/src/tool/bash.rs:45-46, crates/jekko-runtime/src/tool/edit/mod.rs:58-59, crates/jekko-runtime/src/tool/glob.rs:44-45, crates/jekko-runtime/src/tool/grep.rs:44-45, crates/jekko-runtime/src/tool/read.rs:51-52, crates/jekko-runtime/src/tool/task.rs:48-49, crates/jekko-runtime/src/tool/webfetch.rs:90-91, crates/jekko-runtime/src/tool/websearch.rs:79-80, crates/jekko-runtime/src/tool/write.rs:44-45` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 13 | `crates/memory-benchmark/src/bin/cogcore_bench.rs:202-206, crates/memory-benchmark/src/bin/qbank_validate.rs:105-109, crates/memory-benchmark/src/bin/score_mix.rs:157-161` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 8 | `crates/jekko-runtime/src/tool/bash.rs:49-53, crates/jekko-runtime/src/tool/glob.rs:48-52, crates/jekko-runtime/src/tool/grep.rs:48-52` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 1 | `crates/jekko-runner/src/hero_judge/population.rs:147-149, crates/jekko-runner/src/port/plan.rs:163-165, crates/jekko-runner/src/port_runner/config.rs:65-67, crates/qbank-builder/src/fixture.rs:307-309, crates/sandboxctl/src/spec_types.rs:171-173` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 4 | `crates/jekko-runtime/src/tool/edit/mod.rs:105-106, crates/jekko-runtime/src/tool/edit/mod.rs:122-123, crates/jekko-runtime/src/tool/edit/mod.rs:137-138, crates/jekko-runtime/src/tool/glob.rs:79-80, crates/jekko-runtime/src/tool/grep.rs:79-80, crates/jekko-runtime/src/tool/read.rs:105-106, crates/jekko-runtime/src/tool/read.rs:122-123, crates/jekko-runtime/src/tool/write.rs:71-72` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 7 | 18 | `crates/xtask/src/pr_compliance.rs:83-90, crates/xtask/src/pr_standards.rs:157-164` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 7 | 16 | `crates/jekko-runner/src/worker_pool.rs:142-149, crates/jekko-runner/src/worktree.rs:170-177` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 7 | 7 | `crates/zyalc/src/live_audit/report.rs:20-27, crates/zyalc/src/replay_verify.rs:54-61` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 23 | `crates/xtask/src/publish_npm_package.rs:56-62, crates/xtask/src/publish_release_package.rs:154-160` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 13 | `crates/xtask/src/pr_compliance.rs:75-81, crates/xtask/src/pr_standards.rs:149-155` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 1 | `crates/jekko-runner/src/hero_judge/config.rs:103-105, crates/jekko-runner/src/hero_judge/population.rs:159-161, crates/jekko-runner/src/hero_judge/population.rs:163-165, crates/jekko-runner/src/hero_judge/population.rs:167-169` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 5 | 44 | `crates/jekko-store/build.rs:190-195, crates/jekko-store/src/migration.rs:230-235` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/jekko-tui/src/engine/output_collapse.rs:202-203, crates/jekko-tui/src/engine/output_collapse.rs:217-218, crates/jekko-tui/src/engine/output_collapse.rs:236-237, crates/jekko-tui/src/engine/output_collapse.rs:252-253, crates/jekko-tui/src/engine/output_collapse.rs:270-271, crates/jekko-tui/src/engine/output_collapse.rs:289-290` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 3 | `crates/jekko-tui/benches/scroll_100k.rs:25-26, crates/jekko-tui/benches/scroll_100k.rs:37-38, crates/jekko-tui/benches/scroll_100k.rs:47-48, crates/jekko-tui/benches/scroll_100k.rs:71-72, crates/jekko-tui/benches/scroll_100k.rs:87-88, crates/jekko-tui/benches/scroll_100k.rs:104-105` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 5 | 13 | `crates/jekko-tui/src/transcript/terminal_tokenize/matchers.rs:158-163, crates/jekko-tui/src/transcript/yaml_tokenize/recognizers.rs:302-307` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 5 | 12 | `crates/jekko-runner/src/reasoning_io.rs:318-323, crates/jekko-runner/src/reasoning_io.rs:330-335` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 5 | 12 | `crates/jekko-cli/src/cmd/zyal_run.rs:942-947, crates/jekko-runner/src/bounded_queue.rs:182-187` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 5 | 12 | `crates/memory-benchmark/src/bin/score_mix.rs:179-184, crates/memory-benchmark/src/chase_report/snapshot.rs:295-300` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 5 | 11 | `crates/jekko-runner/src/locks.rs:53-58, crates/jekko-runner/src/locks.rs:66-71` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 2 | `crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:68-69, crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:89-90, crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:96-97, crates/zyalc/src/runbook_lint/query.rs:70-71, crates/zyalc/src/runbook_lint/query.rs:125-126, crates/zyalc/src/runbook_lint/query.rs:133-134` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 5 | 8 | `crates/memory-benchmark/src/adapters/reference_claim_skeptic.rs:14-19, crates/memory-benchmark/src/adapters/reference_evidence_ledger.rs:15-20` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 1 | `crates/jekko-provider/src/transform/shared.rs:149-150, crates/jekko-runtime/src/agent/executor.rs:255-256, crates/xtask/src/commands/package.rs:313-314, crates/zyal-core/src/forbidden.rs:107-108, crates/zyal-core/src/forbidden.rs:115-116, crates/zyal-core/src/forbidden.rs:137-138` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/jekko-tui/src/agents/mod.rs:270-271, crates/jekko-tui/src/agents/mod.rs:280-281, crates/jekko-tui/src/agents/mod.rs:295-296, crates/jekko-tui/src/agents/mod.rs:328-329, crates/jekko-tui/src/agents/mod.rs:337-338` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 19 | `crates/xtask/src/close_issues.rs:138-142, crates/xtask/src/compliance_close.rs:194-198` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 18 | `crates/xtask/src/pr_compliance.rs:92-96, crates/xtask/src/pr_management.rs:95-99` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 12 | `crates/xtask/src/pr_compliance.rs:69-73, crates/xtask/src/pr_standards.rs:137-141` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 12 | `crates/zyalc/src/live_audit/tests.rs:10-14, crates/zyalc/src/replay_verify.rs:300-304` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 4 | 8 | `crates/jekko-runner/src/hero_judge_runner_completion.rs:130-134, crates/jekko-runner/src/reasoning_io.rs:342-346` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 4 | `crates/memory-benchmark/src/adapters/reference_claim_skeptic.rs:32-34, crates/memory-benchmark/src/adapters/reference_evidence_ledger.rs:26-28, crates/memory-benchmark/src/candidates/shared.rs:36-38` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 3 | `crates/memory-benchmark/src/adapters/baseline.rs:88-90, crates/memory-benchmark/src/adapters/baseline.rs:92-94, crates/memory-benchmark/src/adapters/baseline.rs:95-97` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 3 | `crates/memory-benchmark/src/adapters/reference_claim_skeptic.rs:62-64, crates/memory-benchmark/src/adapters/reference_evidence_ledger.rs:55-57, crates/memory-benchmark/src/candidates/shared.rs:54-56` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 2 | `crates/jekko-provider/src/providers/jnoccio.rs:28-30, crates/jekko-provider/src/providers/litellm.rs:121-123, crates/jekko-provider/src/providers/openrouter.rs:24-26` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 1 | `crates/jekko-runner/src/hero_judge/population.rs:147-149, crates/jekko-runner/src/port/plan.rs:163-165, crates/qbank-builder/src/fixture.rs:307-309` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 0 | `crates/jekko-runner/src/port_runner/helpers.rs:49-49, crates/jekko-runtime/src/skill.rs:115-116, crates/jekko-runtime/src/tool/mod.rs:224-225, crates/jekko-tui/src/inline_runtime.rs:101-101, crates/memory-benchmark/src/runner.rs:259-259` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/xtask/src/commands/package.rs:256-257, crates/xtask/src/commands/package.rs:270-271, crates/xtask/src/commands/package.rs:280-281, crates/xtask/src/commands/package.rs:298-299` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/jekko-tui/src/prompt/widget.rs:317-318, crates/jekko-tui/src/prompt/widget.rs:329-330, crates/jekko-tui/src/prompt/widget.rs:357-358, crates/jekko-tui/src/prompt/widget.rs:418-419` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 3 | 4 | `crates/jekko-tui/src/transcript/syntax/renderer.rs:147-150, crates/jekko-tui/src/transcript/syntax/renderer.rs:166-169` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 3 | 2 | `crates/jekko-runtime/src/lib.rs:132-135, crates/jekko-runtime/src/session.rs:182-185` | `same-name semantic unit copied across multiple files` |

## Dimensions

| Dimension | Weight | Score | Weighted | Evidence |
| --- | ---: | ---: | ---: | --- |
| Ownership and navigation surface | 13 | 100 | 13.00 | root `AGENTS.md` present; `CODEOWNERS` present |
| Contract and boundary integrity | 13 | 98 | 12.74 | contract surface found; generated contract artifacts found |
| Proof lanes and test routing | 12 | 100 | 12.00 | one-command setup/validation lane found; deterministic fast lane found |
| Security and supply-chain posture | 12 | 86 | 10.32 | lockfile present; secret or dependency scan tooling found |
| Code shape and semantic surface | 12 | 65 | 7.80 | largest authored code file: crates/jekko-cli/src/cmd/zyal_run.rs (986 LOC); code file exceeds 500 LOC |
| Data truth and workflow safety | 8 | 95 | 7.60 | database surface present; structured db boundary manifest present |
| Observability and repair evidence | 8 | 98 | 7.84 | observability libraries or patterns found; diagnostic shaping hints found |
| Context economy and agent instructions | 7 | 100 | 7.00 | root `AGENTS.md` present; root `AGENTS.md` stays short |
| Jankurai tool adoption and CI replacement | 7 | 49 | 3.43 | control-plane files present; applicable=16 |
| Python containment and polyglot hygiene | 4 | 100 | 4.00 | no Python files in scope |
| Build speed signals | 4 | 80 | 3.20 | build acceleration markers found; targeted test/build commands found |

## Reference Profile Structure

- Applicable cells: `7` canonical=`7` noncanonical=`0` guidance missing=`0`

| Cell | Status | Canonical | Detected | Aliases | Guidance | Owner | Proof lane | Agent fix |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `web` | `not_applicable` | `apps/web/` | `-` | `frontend/, ui/, packages/web/, packages/ui/` | `not_required` | `apps/web` | `rendered UX / Playwright` | `no action` |
| `api` | `not_applicable` | `apps/api/` | `-` | `api/, server/, backend/` | `not_required` | `apps/api` | `edge handler / contract tests` | `no action` |
| `domain` | `canonical` | `crates/domain/` | `crates/domain` | `domain/, core/` | `present` | `crates/domain` | `unit / property tests` | `keep `crates/domain/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |
| `application` | `canonical` | `crates/application/` | `crates/application` | `application/, usecases/, use-cases/` | `present` | `crates/application` | `use-case / authz tests` | `keep `crates/application/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |
| `adapters` | `canonical` | `crates/adapters/` | `crates/adapters` | `adapters/, infra/, integrations/` | `present` | `crates/adapters` | `adapter integration tests` | `keep `crates/adapters/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |
| `workers` | `canonical` | `crates/workers/` | `crates/workers` | `workers/, jobs/, scheduler/, queue/` | `present` | `crates/workers` | `workflow / replay tests` | `keep `crates/workers/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |
| `contracts` | `canonical` | `contracts/` | `contracts, generated` | `openapi/, protobuf/, json-schema/, generated/` | `present` | `contracts` | `generation / drift checks` | `keep `contracts/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |
| `db` | `canonical` | `db/` | `db` | `migrations/, constraints/, sql/` | `present` | `db` | `migration / constraint tests` | `keep `db/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |
| `python-ai` | `not_applicable` | `python/ai-service/` | `-` | `python/, ai-service/, evals/, embeddings/, model/` | `not_required` | `python/ai-service` | `eval / contract tests` | `no action` |
| `ops` | `canonical` | `ops/` | `.github, .github/workflows, ops` | `.github/, .github/workflows/, ci/, release/, observability/, security/` | `present` | `ops` | `security lane / workflow lint` | `keep `ops/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |

## Rendered UX QA

- Web surface: `false`
- Layered UX lane: `true`
- Missing: `none`
- Tuiwright TUI flows: `7` flow(s) across `4` file(s); assertions=`14` actions=`10` artifacts=`screenshot=7, trace_path=3`

## Tool Adoption

- Control plane present: `true`
- Applicable tools: `16`
- Configured: `16`
- CI evidence: `5`
- Artifact verified: `0`
- Replaced count: `5`
- Missing CI evidence: `audit-ci, proof-routing, proofbind, proofmark-rust, copy-code, security, ci-bad-behavior, git-bad-behavior, release-bad-behavior, contract-drift, rust-witness, authz-matrix, input-boundary, agent-tool-supply, release-readiness, cost-budget`

| Tool | Category | Mode | Status | Replaced | Artifacts |
| --- | --- | --- | --- | --- | --- |
| `audit-ci` | `audit` | `auto` | `configured` | `manual repo scoring, ad hoc score gates` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `proof-routing` | `proof` | `auto` | `configured` | `ad hoc proof lane selection, manual proof receipts` | `.jankurai/repo-score.json, .jankurai/repo-score.md, target/jankurai/repair-queue.jsonl` |
| `proofbind` | `proof` | `auto` | `ci_evidence` | `manual changed-surface routing, ad hoc proof obligation lists` | `target/jankurai/proofbind/surface-witness.json, target/jankurai/proofbind/obligations.json` |
| `proofmark-rust` | `proof` | `auto` | `configured` | `line-only coverage review, manual in-diff mutation review` | `target/jankurai/proofmark/proofmark-receipt.json, target/jankurai/proofmark/proof-receipt.json` |
| `copy-code` | `audit` | `auto` | `configured` | `ad hoc copy-code review, manual duplication triage` | `target/jankurai/copy-code.json, target/jankurai/copy-code.md` |
| `security` | `security` | `auto` | `configured` | `gitleaks, dependency review, SBOM/provenance` | `target/jankurai/security/evidence.json` |
| `ci-bad-behavior` | `security` | `auto` | `ci_evidence` | `mutable workflow refs, secret echo/debug workflow checks, non-blocking security scans` | `target/jankurai/language-bad-behavior.log` |
| `git-bad-behavior` | `audit` | `auto` | `ci_evidence` | `destructive git automation, force-push release scripts, hidden stash-based state` | `target/jankurai/language-bad-behavior.log` |
| `release-bad-behavior` | `release` | `auto` | `ci_evidence` | `manual release checklist, ad hoc tag and artifact review, manual provenance review` | `target/jankurai/language-bad-behavior.log` |
| `ux-qa` | `ux` | `auto` | `not_applicable` | `playwright, axe-core, visual baselines` | `target/jankurai/ux-qa.json` |
| `db-migration-analyze` | `db` | `auto` | `not_applicable` | `manual migration review` | `target/jankurai/migration-report.json` |
| `contract-drift` | `contract` | `auto` | `configured` | `handwritten contract drift checks, openapi diff` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `rust-witness` | `rust` | `auto` | `ci_evidence` | `manual witness graphing` | `target/jankurai/rust/witness-graph.json` |
| `vibe-coverage` | `audit` | `auto` | `not_applicable` | `manual vibe-coding coverage spreadsheet` | `target/jankurai/vibe-coverage.json, target/jankurai/vibe-coverage.md` |
| `coverage-evidence` | `proof` | `auto` | `not_applicable` | `manual coverage report review, ad hoc mutation survivor review` | `target/jankurai/coverage/coverage-audit.json, target/jankurai/coverage/coverage-audit.md` |
| `authz-matrix` | `security` | `auto` | `configured` | `manual authz matrix review` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `input-boundary` | `security` | `auto` | `configured` | `manual unsafe sink review` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `agent-tool-supply` | `security` | `auto` | `configured` | `manual MCP/tool trust review` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `release-readiness` | `release` | `auto` | `configured` | `manual launch checklist` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |
| `cost-budget` | `release` | `auto` | `configured` | `manual spend review` | `.jankurai/repo-score.json, .jankurai/repo-score.md` |

## Boundary manifest (ingested)

- Path: `agent/boundaries.toml`
- Stack: `rust-ts-postgres-bounded-python` · version: `0.4.0`
- Queue path counts — adapter: `2`, event_contract: `1`, generated_type: `1`, client_marker: `7`, streaming_exception: `1`
- Content fingerprint: `sha256:a7d902610988c389275705c0c130f5879f9aeac7b83ac16291a81de46d861a41`

## Boundary Reclassifications

No audited runtime boundary reclassifications declared.

## Findings

1. `medium` `shape` `.`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:shape` `soft` confidence `0.76`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: `Code shape and semantic surface` scored 65 below the standard floor of 85
   Fix: split large or ambiguous authored code into smaller semantic modules with focused tests
   Rerun: `just fast`
   Fingerprint: `sha256:9229fa4f49c3289ff188ac8fbca08ba643716e5537c569f8ba563e48bbcc1cf0`
   Evidence: largest authored code file: crates/jekko-cli/src/cmd/zyal_run.rs (986 LOC), code file exceeds 500 LOC, most code files stay under 300 LOC, copy-code advisory classes found: 95 (advisory only, no score impact)
2. `medium` `proof` `Justfile`
   Rule: `HLT-018-PERF-CONCURRENCY-DRIFT`
   Check: `HLT-018-PERF-CONCURRENCY-DRIFT:proof` `soft` confidence `0.76`
   Route: TLR `Verification`, lane `fast`, owner `workspace`
   Docs: `docs/testing.md`
   Reason: `Build speed signals` scored 80 below the standard floor of 85
   Fix: add fast deterministic build/test targets, caches, and narrow proof lanes for agent iteration
   Rerun: `just fast`
   Fingerprint: `sha256:2f2531223d7f7036c20d44b58cd52e64aa53ffd6cb85e01e541c1feff0c09cb2`
   Evidence: build acceleration markers found, targeted test/build commands found, locked dependency graph present, CI cache hint found
3. `medium` `governance` `agent/repo-score.json`
   Rule: `HLT-045-GENERATED-ZONE-GOVERNANCE`
   Check: `HLT-045-GENERATED-ZONE-GOVERNANCE:governance` `soft` confidence `0.76`
   Route: TLR `Contracts/data`, lane `contract`, owner `workspace`
   Docs: `agent/JANKURAI_STANDARD.md#generated-zones`
   Reason: generated zone `agent/repo-score.json` has an uncommitted hand-edit at `agent/repo-score.json` instead of a regeneration
   Fix: revert the in-place edit to `agent/repo-score.json` and regenerate it from the declared source/command in `agent/generated-zones.toml`; do not patch generated output by hand
   Rerun: `just fast`
   Fingerprint: `sha256:b39b16db675f2ac6f07bb27c65d75980125641479fb417a52fb0cc618c687610`
   Evidence: `agent/repo-score.json` was hand-edited inside declared generated zone `agent/repo-score.json`
4. `medium` `governance` `agent/repo-score.md`
   Rule: `HLT-045-GENERATED-ZONE-GOVERNANCE`
   Check: `HLT-045-GENERATED-ZONE-GOVERNANCE:governance` `soft` confidence `0.76`
   Route: TLR `Contracts/data`, lane `contract`, owner `workspace`
   Docs: `agent/JANKURAI_STANDARD.md#generated-zones`
   Reason: generated zone `agent/repo-score.md` has an uncommitted hand-edit at `agent/repo-score.md` instead of a regeneration
   Fix: revert the in-place edit to `agent/repo-score.md` and regenerate it from the declared source/command in `agent/generated-zones.toml`; do not patch generated output by hand
   Rerun: `just fast`
   Fingerprint: `sha256:0eb61c1eca345af83abb3815f06460a844f3bcbe63a78fcc212622cf0bc24740`
   Evidence: `agent/repo-score.md` was hand-edited inside declared generated zone `agent/repo-score.md`
5. `medium` `proof` `agent/repo-score.md:272`
   Rule: `HLT-027-HUMAN-REVIEW-EVIDENCE-GAP`
   Check: `HLT-027-HUMAN-REVIEW-EVIDENCE-GAP:proof` `soft` confidence `0.88`
   Route: TLR `Repair`, lane `audit`, owner `workspace`
   Docs: `docs/testing.md`
   Matched term: `review evidence`
   Reason: proof and review claims need receipts
   Fix: attach raw CI logs, review receipts, and replayable commands instead of accepting claims or summaries
   Rerun: `just score`
   Fingerprint: `sha256:6b56209aa2cbaf288a5e33f3ea79eaa096a0b3982a23778a19a73e78ac3de1e1`
   Evidence: Evidence: Evidence: /// the resolution ratio penalizes fabricated references — a candidate that
6. `medium` `copy-code` `crates/jekko-runner/src/bounded_queue.rs:25`
   Rule: `HLT-046-UNNECESSARY-VARIETY`
   Check: `HLT-046-UNNECESSARY-VARIETY:copy-code` `soft` confidence `0.88`
   Route: TLR `Maintainability entropy`, lane `copy-code`, owner `tools`
   Docs: `agent/JANKURAI_STANDARD.md#jankurai-pillar-variety-and-canonical-shape`
   Matched term: `unnecessary-variety`
   Reason: enum `JobStatus` has 2 divergent definitions across modules where one consistent definition is expected
   Fix: define `JobStatus` once in a shared module and import it everywhere, or reconcile the diverging definitions so one canonical shape is used; redundant variety lets the copies drift apart
   Rerun: `cargo run -p jankurai -- copy-code . --json target/jankurai/copy-code.json --md target/jankurai/copy-code.md`
   Fingerprint: `sha256:8152d31307d4ff79bb17c91b40cd50fda62d4fd7e0b9bc113e4e27e483883d75`
   Evidence: enum `JobStatus` is defined with diverging shapes in 2 modules (crates/jekko-runner/src/bounded_queue.rs:25, crates/jekko-tui/src/background.rs:50)
7. `medium` `copy-code` `crates/jekko-runner/src/research.rs:31`
   Rule: `HLT-046-UNNECESSARY-VARIETY`
   Check: `HLT-046-UNNECESSARY-VARIETY:copy-code` `soft` confidence `0.88`
   Route: TLR `Maintainability entropy`, lane `copy-code`, owner `tools`
   Docs: `agent/JANKURAI_STANDARD.md#jankurai-pillar-variety-and-canonical-shape`
   Matched term: `unnecessary-variety`
   Reason: enum `ClaimStatus` has 2 divergent definitions across modules where one consistent definition is expected
   Fix: define `ClaimStatus` once in a shared module and import it everywhere, or reconcile the diverging definitions so one canonical shape is used; redundant variety lets the copies drift apart
   Rerun: `cargo run -p jankurai -- copy-code . --json target/jankurai/copy-code.json --md target/jankurai/copy-code.md`
   Fingerprint: `sha256:e8cead23d36c3f479354fc99c3b8f11ec3dd85761666ea85cc50ea98f2f2ae08`
   Evidence: enum `ClaimStatus` is defined with diverging shapes in 2 modules (crates/jekko-runner/src/research.rs:31, crates/memory-benchmark/src/result.rs:39)
8. `medium` `copy-code` `crates/jekko-runner/src/watcher/guards.rs:15`
   Rule: `HLT-046-UNNECESSARY-VARIETY`
   Check: `HLT-046-UNNECESSARY-VARIETY:copy-code` `soft` confidence `0.88`
   Route: TLR `Maintainability entropy`, lane `copy-code`, owner `tools`
   Docs: `agent/JANKURAI_STANDARD.md#jankurai-pillar-variety-and-canonical-shape`
   Matched term: `unnecessary-variety`
   Reason: enum `GuardMode` has 2 divergent definitions across modules where one consistent definition is expected
   Fix: define `GuardMode` once in a shared module and import it everywhere, or reconcile the diverging definitions so one canonical shape is used; redundant variety lets the copies drift apart
   Rerun: `cargo run -p jankurai -- copy-code . --json target/jankurai/copy-code.json --md target/jankurai/copy-code.md`
   Fingerprint: `sha256:f30b28536b3f32ad864579243f0895795ea880c15261d8f80ea0a35ca1bc7d52`
   Evidence: enum `GuardMode` is defined with diverging shapes in 2 modules (crates/jekko-runner/src/watcher/guards.rs:15, crates/xtask/src/runtime_checks.rs:10)
9. `medium` `governance` `generated/ir/flowgraph.schema.json`
   Rule: `HLT-045-GENERATED-ZONE-GOVERNANCE`
   Check: `HLT-045-GENERATED-ZONE-GOVERNANCE:governance` `soft` confidence `0.76`
   Route: TLR `Contracts/data`, lane `contract`, owner `agent`
   Docs: `agent/JANKURAI_STANDARD.md#generated-zones`
   Reason: generated zone `generated/ir/flowgraph.schema.json` has an uncommitted hand-edit at `generated/ir/flowgraph.schema.json` instead of a regeneration
   Fix: revert the in-place edit to `generated/ir/flowgraph.schema.json` and regenerate it from the declared source/command in `agent/generated-zones.toml`; do not patch generated output by hand
   Rerun: `just fast`
   Fingerprint: `sha256:d90f6e4a7b37b6546b56cdf9880312a346d17e0f9f2b87138c5ea1ba130b7dd2`
   Evidence: `generated/ir/flowgraph.schema.json` was hand-edited inside declared generated zone `generated/ir/flowgraph.schema.json`
10. `medium` `governance` `generated/ir/zyal.d.ts`
   Rule: `HLT-045-GENERATED-ZONE-GOVERNANCE`
   Check: `HLT-045-GENERATED-ZONE-GOVERNANCE:governance` `soft` confidence `0.76`
   Route: TLR `Contracts/data`, lane `contract`, owner `agent`
   Docs: `agent/JANKURAI_STANDARD.md#generated-zones`
   Reason: generated zone `generated/ir/zyal.d.ts` has an uncommitted hand-edit at `generated/ir/zyal.d.ts` instead of a regeneration
   Fix: revert the in-place edit to `generated/ir/zyal.d.ts` and regenerate it from the declared source/command in `agent/generated-zones.toml`; do not patch generated output by hand
   Rerun: `just fast`
   Fingerprint: `sha256:abadae5e5e94c0c9a2db727e34eb9b725810eef7c87d41a2a28cceb7f4843d60`
   Evidence: `generated/ir/zyal.d.ts` was hand-edited inside declared generated zone `generated/ir/zyal.d.ts`

## Policy

- Policy file: `./agent/audit-policy.toml`
- Minimum score: `85`
- Fail on: `critical, high`

## Agent Fix Queue

1. `medium` `HLT-045-GENERATED-ZONE-GOVERNANCE` `agent/repo-score.json` - revert the in-place edit to `agent/repo-score.json` and regenerate it from the declared source/command in `agent/generated-zones.toml`; do not patch generated output by hand
   Route: `Contracts/data`/`contract`
2. `medium` `HLT-045-GENERATED-ZONE-GOVERNANCE` `agent/repo-score.md` - revert the in-place edit to `agent/repo-score.md` and regenerate it from the declared source/command in `agent/generated-zones.toml`; do not patch generated output by hand
   Route: `Contracts/data`/`contract`
3. `medium` `HLT-045-GENERATED-ZONE-GOVERNANCE` `generated/ir/flowgraph.schema.json` - revert the in-place edit to `generated/ir/flowgraph.schema.json` and regenerate it from the declared source/command in `agent/generated-zones.toml`; do not patch generated output by hand
   Route: `Contracts/data`/`contract`
4. `medium` `HLT-045-GENERATED-ZONE-GOVERNANCE` `generated/ir/zyal.d.ts` - revert the in-place edit to `generated/ir/zyal.d.ts` and regenerate it from the declared source/command in `agent/generated-zones.toml`; do not patch generated output by hand
   Route: `Contracts/data`/`contract`
5. `medium` `HLT-018-PERF-CONCURRENCY-DRIFT` `Justfile` - add fast deterministic build/test targets, caches, and narrow proof lanes for agent iteration
   Route: `Verification`/`fast`
6. `medium` `HLT-027-HUMAN-REVIEW-EVIDENCE-GAP` `agent/repo-score.md` - attach raw CI logs, review receipts, and replayable commands instead of accepting claims or summaries
   Route: `Repair`/`audit`
7. `medium` `HLT-001-DEAD-MARKER` `.` - split large or ambiguous authored code into smaller semantic modules with focused tests
   Route: `Entropy`/`fast`
8. `medium` `HLT-046-UNNECESSARY-VARIETY` `crates/jekko-runner/src/bounded_queue.rs` - define `JobStatus` once in a shared module and import it everywhere, or reconcile the diverging definitions so one canonical shape is used; redundant variety lets the copies drift apart
   Route: `Maintainability entropy`/`copy-code`
9. `medium` `HLT-046-UNNECESSARY-VARIETY` `crates/jekko-runner/src/research.rs` - define `ClaimStatus` once in a shared module and import it everywhere, or reconcile the diverging definitions so one canonical shape is used; redundant variety lets the copies drift apart
   Route: `Maintainability entropy`/`copy-code`
10. `medium` `HLT-046-UNNECESSARY-VARIETY` `crates/jekko-runner/src/watcher/guards.rs` - define `GuardMode` once in a shared module and import it everywhere, or reconcile the diverging definitions so one canonical shape is used; redundant variety lets the copies drift apart
   Route: `Maintainability entropy`/`copy-code`
