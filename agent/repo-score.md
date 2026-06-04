# jankurai Repo Score

- Standard: `jankurai`
- Auditor: `1.5.1`
- Schema: `1.9.0`
- Paper edition: `2026.05-ed8`
- Target stack ID: `rust-ts-vite-react-postgres-bounded-python`
- Target stack: `Rust core + TypeScript/React/Vite + PostgreSQL + generated contracts + exception-only Python AI/data service`
- Repo: `.`
- Run ID: `1780114285`
- Started at: `1780114285`
- Elapsed: `14571` ms
- Scope: `full`
- Raw score: `83`
- Final score: `64`
- Decision: `advisory`
- Minimum score: `85`
- Caps applied: `vibe-placeholders-in-product-code, fallback-soup-in-product-code, future-hostile-dead-language-in-product-code, generated-zone-mutation-risk, agent-tool-supply-chain-gap, rust-bad-behavior`

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
| `vibe-placeholders-in-product-code` | 68 | yes |
| `fallback-soup-in-product-code` | 70 | yes |
| `future-hostile-dead-language-in-product-code` | 64 | yes |
| `severe-duplication-in-product-code` | 70 | no |
| `generated-zone-mutation-risk` | 76 | yes |
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
| `agent-tool-supply-chain-gap` | 78 | yes |
| `release-readiness-gap` | 80 | no |
| `missing-rust-property-or-integration-tests` | 82 | no |
| `no-agent-friendly-exception-pattern` | 76 | no |
| `missing-agent-readable-docs` | 80 | no |
| `streaming-runtime-drift` | 78 | no |
| `rust-bad-behavior` | 72 | yes |
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

- Status: `review` hard=`0` warning=`91` files=`924`
- Policy: min-lines=`10` min-tokens=`100` max-findings=`50` include-tests=`false` strict=`false`
- Duplicate volume: lines=`266` tokens=`766` bytes=`7228`

- Notes:
  - hard classes are limited to exact active-source file matches and substantial exact same-name units
  - warning classes include same-body different-name units and token/block duplication
  - tests, fixtures, stories, config, Docker, and migrations are omitted unless --include-tests is set
  - showing the top 50 classes and omitting 41 lower-ranked classes

| Kind | Severity | Language | Lines | Tokens | Instances | Reason |
| --- | --- | --- | ---: | ---: | --- | --- |
| `ExactUnitDifferentName` | `Warning` | `rust` | 24 | 86 | `crates/jekko-runtime/src/file.rs:140-164, crates/jekko-runtime/src/permission.rs:269-293` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 8 | `crates/jekko-runtime/src/tool/edit/mod.rs:62-66, crates/jekko-runtime/src/tool/read.rs:55-59, crates/jekko-runtime/src/tool/task.rs:52-56, crates/jekko-runtime/src/tool/webfetch.rs:94-98, crates/jekko-runtime/src/tool/websearch.rs:83-87, crates/jekko-runtime/src/tool/write.rs:48-52` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 17 | 54 | `crates/memory-benchmark/src/corpus/real_papers/model.rs:219-236, crates/qbank-builder/src/core_types.rs:80-97` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 1 | `crates/memory-benchmark/src/candidates/arena/lane_08.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_09.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_10.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_11.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_12.rs:17-19, crates/memory-benchmark/src/candidates/arena/lane_13.rs:17-19, crates/memory-benchmark/src/candidates/arena/lane_14.rs:17-19, crates/memory-benchmark/src/candidates/arena/lane_15.rs:17-19` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 1 | `crates/memory-benchmark/src/candidates/arena/lane_04.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_05.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_06.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_07.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_16.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_17.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_18.rs:16-18, crates/memory-benchmark/src/candidates/arena/lane_19.rs:16-18` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 12 | `crates/xtask/src/close_issues.rs:127-131, crates/xtask/src/compliance_close.rs:183-187, crates/xtask/src/pr_compliance.rs:146-150, crates/xtask/src/pr_standards.rs:143-147` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 18 | `crates/xtask/src/publish_release.rs:91-97, crates/xtask/src/publish_release_package.rs:183-189, crates/xtask/src/publish_release_registry.rs:229-235` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 12 | 33 | `crates/jekko-store/build.rs:197-209, crates/jekko-store/src/migration.rs:237-249` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 10 | 31 | `crates/xtask/src/publish_npm_package.rs:44-54, crates/xtask/src/publish_release_package.rs:142-152` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 3 | 14 | `crates/xtask/src/commands/security_lane.rs:189-192, crates/xtask/src/commands/security_lane.rs:223-226, crates/xtask/src/commands/security_lane.rs:245-248, crates/xtask/src/commands/security_lane.rs:273-276` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 9 | 15 | `crates/jankurai-runner/src/bin_main/hero_series/files.rs:25-34, crates/jankurai-runner/src/hero_judge_runner_helpers.rs:132-141` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 1 | 4 | `crates/jekko-runtime/src/tool/bash.rs:45-46, crates/jekko-runtime/src/tool/edit/mod.rs:58-59, crates/jekko-runtime/src/tool/glob.rs:44-45, crates/jekko-runtime/src/tool/grep.rs:44-45, crates/jekko-runtime/src/tool/read.rs:51-52, crates/jekko-runtime/src/tool/task.rs:48-49, crates/jekko-runtime/src/tool/webfetch.rs:90-91, crates/jekko-runtime/src/tool/websearch.rs:79-80, crates/jekko-runtime/src/tool/write.rs:44-45` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 13 | `crates/memory-benchmark/src/bin/cogcore_bench.rs:202-206, crates/memory-benchmark/src/bin/qbank_validate.rs:105-109, crates/memory-benchmark/src/bin/score_mix.rs:157-161` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 8 | `crates/jekko-runtime/src/tool/bash.rs:49-53, crates/jekko-runtime/src/tool/glob.rs:48-52, crates/jekko-runtime/src/tool/grep.rs:48-52` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 1 | `crates/jankurai-runner/src/hero_judge/population.rs:140-142, crates/jankurai-runner/src/port/plan.rs:161-163, crates/jankurai-runner/src/port_runner/config.rs:65-67, crates/qbank-builder/src/fixture.rs:307-309, crates/sandboxctl/src/spec_types.rs:169-171` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 4 | `crates/jekko-runtime/src/tool/edit/mod.rs:105-106, crates/jekko-runtime/src/tool/edit/mod.rs:122-123, crates/jekko-runtime/src/tool/edit/mod.rs:137-138, crates/jekko-runtime/src/tool/glob.rs:79-80, crates/jekko-runtime/src/tool/grep.rs:79-80, crates/jekko-runtime/src/tool/read.rs:105-106, crates/jekko-runtime/src/tool/read.rs:122-123, crates/jekko-runtime/src/tool/write.rs:71-72` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 7 | 18 | `crates/xtask/src/pr_compliance.rs:83-90, crates/xtask/src/pr_standards.rs:157-164` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 7 | 16 | `crates/jankurai-runner/src/worker_pool.rs:142-149, crates/jankurai-runner/src/worktree.rs:170-177` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 7 | 7 | `crates/zyalc/src/live_audit/report.rs:20-27, crates/zyalc/src/replay_verify.rs:54-61` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 23 | `crates/xtask/src/publish_npm_package.rs:56-62, crates/xtask/src/publish_release_package.rs:154-160` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 13 | `crates/xtask/src/pr_compliance.rs:75-81, crates/xtask/src/pr_standards.rs:149-155` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 1 | `crates/jankurai-runner/src/hero_judge/config.rs:103-105, crates/jankurai-runner/src/hero_judge/population.rs:152-154, crates/jankurai-runner/src/hero_judge/population.rs:156-158, crates/jankurai-runner/src/hero_judge/population.rs:160-162` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 5 | 44 | `crates/jekko-store/build.rs:190-195, crates/jekko-store/src/migration.rs:230-235` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/jekko-tui/src/engine/output_collapse.rs:202-203, crates/jekko-tui/src/engine/output_collapse.rs:217-218, crates/jekko-tui/src/engine/output_collapse.rs:236-237, crates/jekko-tui/src/engine/output_collapse.rs:252-253, crates/jekko-tui/src/engine/output_collapse.rs:270-271, crates/jekko-tui/src/engine/output_collapse.rs:289-290` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 3 | `crates/jekko-tui/benches/scroll_100k.rs:25-26, crates/jekko-tui/benches/scroll_100k.rs:37-38, crates/jekko-tui/benches/scroll_100k.rs:47-48, crates/jekko-tui/benches/scroll_100k.rs:71-72, crates/jekko-tui/benches/scroll_100k.rs:87-88, crates/jekko-tui/benches/scroll_100k.rs:104-105` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 5 | 13 | `crates/jekko-tui/src/transcript/terminal_tokenize/matchers.rs:158-163, crates/jekko-tui/src/transcript/yaml_tokenize/recognizers.rs:302-307` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 5 | 12 | `crates/jankurai-runner/src/reasoning_io.rs:318-323, crates/jankurai-runner/src/reasoning_io.rs:330-335` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 5 | 11 | `crates/jankurai-runner/src/locks.rs:53-58, crates/jankurai-runner/src/locks.rs:66-71` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 2 | `crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:68-69, crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:89-90, crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:96-97, crates/zyalc/src/runbook_lint/query.rs:70-71, crates/zyalc/src/runbook_lint/query.rs:125-126, crates/zyalc/src/runbook_lint/query.rs:133-134` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 5 | 8 | `crates/memory-benchmark/src/adapters/reference_claim_skeptic.rs:14-19, crates/memory-benchmark/src/adapters/reference_evidence_ledger.rs:15-20` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 1 | `crates/jekko-provider/src/transform/shared.rs:149-150, crates/jekko-runtime/src/agent/executor.rs:255-256, crates/xtask/src/commands/package.rs:313-314, crates/zyal-core/src/forbidden.rs:107-108, crates/zyal-core/src/forbidden.rs:115-116, crates/zyal-core/src/forbidden.rs:137-138` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/jekko-tui/src/agents/mod.rs:270-271, crates/jekko-tui/src/agents/mod.rs:280-281, crates/jekko-tui/src/agents/mod.rs:295-296, crates/jekko-tui/src/agents/mod.rs:328-329, crates/jekko-tui/src/agents/mod.rs:337-338` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 19 | `crates/xtask/src/close_issues.rs:138-142, crates/xtask/src/compliance_close.rs:194-198` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 18 | `crates/xtask/src/pr_compliance.rs:92-96, crates/xtask/src/pr_management.rs:95-99` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 12 | `crates/xtask/src/pr_compliance.rs:69-73, crates/xtask/src/pr_standards.rs:137-141` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 12 | `crates/zyalc/src/live_audit/tests.rs:10-14, crates/zyalc/src/replay_verify.rs:300-304` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 4 | 8 | `crates/jankurai-runner/src/hero_judge_runner_completion.rs:126-130, crates/jankurai-runner/src/reasoning_io.rs:342-346` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 4 | `crates/memory-benchmark/src/adapters/reference_claim_skeptic.rs:32-34, crates/memory-benchmark/src/adapters/reference_evidence_ledger.rs:26-28, crates/memory-benchmark/src/candidates/shared.rs:36-38` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 3 | `crates/memory-benchmark/src/adapters/baseline.rs:88-90, crates/memory-benchmark/src/adapters/baseline.rs:92-94, crates/memory-benchmark/src/adapters/baseline.rs:95-97` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 3 | `crates/memory-benchmark/src/adapters/reference_claim_skeptic.rs:62-64, crates/memory-benchmark/src/adapters/reference_evidence_ledger.rs:55-57, crates/memory-benchmark/src/candidates/shared.rs:54-56` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 2 | `crates/jekko-provider/src/providers/jnoccio.rs:28-30, crates/jekko-provider/src/providers/litellm.rs:121-123, crates/jekko-provider/src/providers/openrouter.rs:24-26` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 1 | `crates/jankurai-runner/src/hero_judge/population.rs:140-142, crates/jankurai-runner/src/port/plan.rs:161-163, crates/qbank-builder/src/fixture.rs:307-309` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 0 | `crates/jankurai-runner/src/port_runner/helpers.rs:49-49, crates/jekko-runtime/src/skill.rs:115-116, crates/jekko-runtime/src/tool/mod.rs:224-225, crates/jekko-tui/src/inline_runtime.rs:101-101, crates/memory-benchmark/src/runner.rs:259-259` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/xtask/src/commands/package.rs:256-257, crates/xtask/src/commands/package.rs:270-271, crates/xtask/src/commands/package.rs:280-281, crates/xtask/src/commands/package.rs:298-299` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/jekko-tui/src/prompt/widget.rs:317-318, crates/jekko-tui/src/prompt/widget.rs:329-330, crates/jekko-tui/src/prompt/widget.rs:357-358, crates/jekko-tui/src/prompt/widget.rs:418-419` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 3 | 4 | `crates/jekko-tui/src/transcript/syntax/renderer.rs:147-150, crates/jekko-tui/src/transcript/syntax/renderer.rs:166-169` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 3 | 2 | `crates/jekko-runtime/src/lib.rs:132-135, crates/jekko-runtime/src/session.rs:182-185` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 7 | `crates/jekko-tui/src/osc52.rs:131-132, crates/jekko-tui/src/osc52.rs:141-142, crates/jekko-tui/src/osc52.rs:149-150` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/memory-benchmark/src/adapters/cogcore_adapter.rs:311-312, crates/memory-benchmark/src/adapters/cogcore_adapter.rs:325-326, crates/memory-benchmark/src/adapters/cogcore_adapter.rs:336-337` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 9 | `crates/jekko-store/build.rs:186-188, crates/jekko-store/src/migration.rs:226-228` | `same-name semantic unit copied across multiple files` |

## Dimensions

| Dimension | Weight | Score | Weighted | Evidence |
| --- | ---: | ---: | ---: | --- |
| Ownership and navigation surface | 13 | 100 | 13.00 | root `AGENTS.md` present; `CODEOWNERS` present |
| Contract and boundary integrity | 13 | 98 | 12.74 | contract surface found; generated contract artifacts found |
| Proof lanes and test routing | 12 | 100 | 12.00 | one-command setup/validation lane found; deterministic fast lane found |
| Security and supply-chain posture | 12 | 94 | 11.28 | lockfile present; secret or dependency scan tooling found |
| Code shape and semantic surface | 12 | 0 | 0.00 | largest authored code file: crates/memory-benchmark/src/fixture/data.rs (2557 LOC); code file exceeds 500 LOC |
| Data truth and workflow safety | 8 | 95 | 7.60 | database surface present; structured db boundary manifest present |
| Observability and repair evidence | 8 | 98 | 7.84 | observability libraries or patterns found; diagnostic shaping hints found |
| Context economy and agent instructions | 7 | 100 | 7.00 | root `AGENTS.md` present; root `AGENTS.md` stays short |
| Jankurai tool adoption and CI replacement | 7 | 49 | 3.43 | control-plane files present; applicable=16 |
| Python containment and polyglot hygiene | 4 | 100 | 4.00 | no Python files in scope |
| Build speed signals | 4 | 95 | 3.80 | build acceleration markers found; targeted test/build commands found |

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
| `contracts` | `canonical` | `contracts/` | `contracts` | `openapi/, protobuf/, json-schema/, generated/` | `present` | `contracts` | `generation / drift checks` | `keep `contracts/AGENTS.md` aligned with owns / forbidden / proof lane guidance` |
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
   Reason: `Code shape and semantic surface` scored 0 below the standard floor of 85
   Fix: split large or ambiguous authored code into smaller semantic modules with focused tests
   Rerun: `just fast`
   Fingerprint: `sha256:43c051648e5ad9a7abab39e3459675c52c7db75ef807709127066ad60a90582e`
   Evidence: largest authored code file: crates/memory-benchmark/src/fixture/data.rs (2557 LOC), code file exceeds 500 LOC, code file exceeds 1000 LOC, most code files stay under 300 LOC
2. `high` `generated` `agent/generated-zones.toml:1`
   Rule: `HLT-002-GENERATED-MUTATION`
   Check: `HLT-002-GENERATED-MUTATION:generated` `hard` confidence `0.95`
   Route: TLR `Contracts/data`, lane `contract`, owner `workspace`
   Docs: `agent/JANKURAI_STANDARD.md#generated-zones`
   Reason: generated zone is not protected strongly enough against hand edits
   Fix: add `agent/generated-zones.toml`, require generated/do-not-edit markers, and route repairs to the source contract
   Rerun: `just fast`
   Fingerprint: `sha256:944fb8ea13fac99bc4d88599306b0824999856bd7749785448443a4ec07984d7`
   Evidence: generated zone declaration `agent/sandbox-lanes.toml` targets protected source or control-plane code
3. `high` `security` `agent/zyal/ambitious-superworkflow.zyal:387`
   Rule: `HLT-024-AGENT-TOOL-SUPPLY-GAP`
   Check: `HLT-024-AGENT-TOOL-SUPPLY-GAP:security` `hard` confidence `0.88`
   Route: TLR `Security, secrets, agency`, lane `security`, owner `agent`
   Docs: `docs/audit-rubric.md#top-level-risk-mapping`
   Matched term: `ZYAL_ARM`
   Reason: missing arm
   Fix: append `ZYAL_ARM RUN_FOREVER id=<id>` on the final line
   Rerun: `just security`
   Fingerprint: `sha256:821aafc4672d2bf6d71f6d817435f9b4f9ba6cacd56aa8c796d8bb5a1ddfa2e4`
   Evidence: open_id=ambitious-superworkflow-template
4. `high` `security` `agent/zyal/sandbox-lanes.zyal:3`
   Rule: `HLT-024-AGENT-TOOL-SUPPLY-GAP`
   Check: `HLT-024-AGENT-TOOL-SUPPLY-GAP:security` `hard` confidence `0.88`
   Route: TLR `Security, secrets, agency`, lane `security`, owner `agent`
   Docs: `docs/audit-rubric.md#top-level-risk-mapping`
   Matched term: `<<<ZYAL`
   Reason: non-open sentinel
   Fix: keep the runbook envelope at the top of the file after optional comments
   Rerun: `just security`
   Fingerprint: `sha256:b2a299af938e370b48e782c2a21ac41d91eb6a027364994144504e2a12b4894e`
   Evidence: supported_contract_version=2.4.0, runtime_sentinel_version=v1
5. `high` `security` `crates/jekko-runtime/src/agent/provider.rs:145`
   Rule: `HLT-029-RUST-BAD-BEHAVIOR`
   Check: `HLT-029-RUST-BAD-BEHAVIOR:security` `hard` confidence `0.95`
   Route: TLR `Security, secrets, agency`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#top-level-risk-mapping`
   Matched term: `rust.unsafe.static-mut`
   Reason: global mutation proof is missing
   Fix: replace the mutable static with explicit synchronization or scoped ownership
   Rerun: `just fast`
   Fingerprint: `sha256:4bbd50117d51b8864f533a622b6a06774deda2f952a4397bfef2439a4c335cf6`
   Evidence: detector=static mut, proof-window=NearbySafetyComment, snippet=fn balancer() -> &'static Mutex<Option<KeyBalancer>> {
6. `high` `vibe` `crates/jekko-runtime/src/daemon.rs:231`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: fallback soup detected in product code
   Fix: collapse fallback chains into explicit typed states with bounded retry policy, telemetry, and documented repair guidance
   Rerun: `just fast`
   Fingerprint: `sha256:a2080a15cdd4c06e7eb7e18334ca6813ce6fb2c9a8fe3786e04b75a085ddb27e`
   Evidence: crates/jekko-runtime/src/daemon.rs:231 serde_json::to_value(record).unwrap_or_else(|_| serde_json::json!({})),
7. `high` `vibe` `crates/memory-benchmark/src/adapters/reference_context_pack.rs:142`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `deprecated` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:4fc563586ba63a80b831f60984df89eaf87bd0459b608aa8e57590b3993e7c56`
   Evidence: crates/memory-benchmark/src/adapters/reference_context_pack.rs:142, future-hostile/dead-language term `deprecated` appears
8. `medium` `proof` `crates/memory-benchmark/src/chase_report.rs:39`
   Rule: `HLT-027-HUMAN-REVIEW-EVIDENCE-GAP`
   Check: `HLT-027-HUMAN-REVIEW-EVIDENCE-GAP:proof` `soft` confidence `0.88`
   Route: TLR `Repair`, lane `audit`, owner `tools`
   Docs: `docs/testing.md`
   Matched term: `review evidence`
   Reason: proof and review claims need receipts
   Fix: attach raw CI logs, review receipts, and replayable commands instead of accepting claims or summaries
   Rerun: `just score`
   Fingerprint: `sha256:423b3c5e9f34e3a76640a8101ac1746c1acfc060b044164a7fff0da274caab6b`
   Evidence: pub fabricated_citations: u32,
9. `high` `security` `crates/memory-benchmark/src/chase_report.rs:1431`
   Rule: `HLT-029-RUST-BAD-BEHAVIOR`
   Check: `HLT-029-RUST-BAD-BEHAVIOR:security` `hard` confidence `0.95`
   Route: TLR `Security, secrets, agency`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#top-level-risk-mapping`
   Matched term: `rust.unsafe.undocumented-block`
   Reason: no nearby SAFETY comment was found
   Fix: add a precise `SAFETY:` comment or remove the unsafe block
   Rerun: `just fast`
   Fingerprint: `sha256:b566690531c444a943081f587f64719af1ff3e839453f7e4f776c5c8a788d510`
   Evidence: detector=unsafe {, proof-window=NearbySafetyComment, snippet=" unsafe{",
10. `high` `vibe` `crates/memory-benchmark/src/chase_report.rs:1433`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: product code contains TODO/stub/unimplemented/unreachable placeholder markers
   Fix: replace placeholders with implemented behavior, typed unsupported-state errors, or a tracked exception record with docs
   Rerun: `just fast`
   Fingerprint: `sha256:32446b812827c6375ed59c75267d53f335b77865abd5700593408b3474257166`
   Evidence: crates/memory-benchmark/src/chase_report.rs:1433 "unimplemented!(",
11. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:163`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `deprecated` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:83cac91ad2e5b770073e4e26a26ce0f8ef24b0643ef1f4fd342338f8426c61fc`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:163, future-hostile/dead-language term `deprecated` appears
12. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:172`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `deprecated` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:637f3d961cda4e96d2019b7b23df1f8cb79715d41e72c0766af8448c290ac3d8`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:172, future-hostile/dead-language term `deprecated` appears
13. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:1102`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `legacy` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:36577893d981c09f063e5bd50b78c413fc00150d1a68a5c93e41149020222fc3`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:1102, future-hostile/dead-language term `legacy` appears
14. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:1851`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `stale` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:d6bbb11f209b912f1b01570c65a172edda3febbb8bc95c6da8ca8f33afd0a37b`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:1851, future-hostile/dead-language term `stale` appears
15. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:1866`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `deprecated` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:3417e9ed6e6f1ca0f83bafd0abb59b3a87053ddf9558c7e5d78563495b4e1ae4`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:1866, future-hostile/dead-language term `deprecated` appears
16. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:1867`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `stale` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:d7337d66e5473f897d8705f6730144a41dd0f6fa9a4e9d749172703829fe0d8b`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:1867, future-hostile/dead-language term `stale` appears
17. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:1990`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `old` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:1b7e4ef517475da560005d2a483365ed5e5c40e3e4d6b7a3b5383981113ca6df`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:1990, future-hostile/dead-language term `old` appears
18. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:2044`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `deprecated` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:c1cc167baf9cdfd1a9b9391c9bcba510fda04a23ab16a8c0e333b706ef1f5893`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:2044, future-hostile/dead-language term `deprecated` appears
19. `high` `vibe` `crates/memory-benchmark/src/fixture/data.rs:2045`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `stale` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:a31990e16d0dcb3a39a63d9afaa4c6f7e33df28eef14a4ad8900dc89134018cb`
   Evidence: crates/memory-benchmark/src/fixture/data.rs:2045, future-hostile/dead-language term `stale` appears
20. `high` `vibe` `crates/memory-benchmark/src/scorer.rs:125`
   Rule: `HLT-001-DEAD-MARKER`
   Check: `HLT-001-DEAD-MARKER:vibe` `hard` confidence `0.88`
   Route: TLR `Entropy`, lane `fast`, owner `tools`
   Docs: `docs/audit-rubric.md#future-hostile-language-rule`
   Reason: future-hostile/dead-language term `stale` appears in product/runtime code
   Fix: remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Rerun: `just fast`
   Fingerprint: `sha256:474199b89d64086dcca22341c7d3145f3013d582154f294a475295d673f17ac7`
   Evidence: crates/memory-benchmark/src/scorer.rs:125, future-hostile/dead-language term `stale` appears

## Policy

- Policy file: `./agent/audit-policy.toml`
- Minimum score: `85`
- Fail on: `critical, high`

## Agent Fix Queue

1. `high` `HLT-002-GENERATED-MUTATION` `agent/generated-zones.toml` - add `agent/generated-zones.toml`, require generated/do-not-edit markers, and route repairs to the source contract
   Route: `Contracts/data`/`contract`
2. `medium` `HLT-027-HUMAN-REVIEW-EVIDENCE-GAP` `crates/memory-benchmark/src/chase_report.rs` - attach raw CI logs, review receipts, and replayable commands instead of accepting claims or summaries
   Route: `Repair`/`audit`
3. `high` `HLT-024-AGENT-TOOL-SUPPLY-GAP` `agent/zyal/ambitious-superworkflow.zyal` - append `ZYAL_ARM RUN_FOREVER id=<id>` on the final line
   Route: `Security, secrets, agency`/`security`
4. `high` `HLT-024-AGENT-TOOL-SUPPLY-GAP` `agent/zyal/sandbox-lanes.zyal` - keep the runbook envelope at the top of the file after optional comments
   Route: `Security, secrets, agency`/`security`
5. `high` `HLT-029-RUST-BAD-BEHAVIOR` `crates/jekko-runtime/src/agent/provider.rs` - replace the mutable static with explicit synchronization or scoped ownership
   Route: `Security, secrets, agency`/`fast`
6. `high` `HLT-001-DEAD-MARKER` `crates/jekko-runtime/src/daemon.rs` - collapse fallback chains into explicit typed states with bounded retry policy, telemetry, and documented repair guidance
   Route: `Entropy`/`fast`
7. `high` `HLT-001-DEAD-MARKER` `crates/memory-benchmark/src/adapters/reference_context_pack.rs` - remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Route: `Entropy`/`fast`
8. `high` `HLT-029-RUST-BAD-BEHAVIOR` `crates/memory-benchmark/src/chase_report.rs` - add a precise `SAFETY:` comment or remove the unsafe block
   Route: `Security, secrets, agency`/`fast`
9. `high` `HLT-001-DEAD-MARKER` `crates/memory-benchmark/src/chase_report.rs` - replace placeholders with implemented behavior, typed unsupported-state errors, or a tracked exception record with docs
   Route: `Entropy`/`fast`
10. `high` `HLT-001-DEAD-MARKER` `crates/memory-benchmark/src/fixture/data.rs` - remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Route: `Entropy`/`fast`
11. `high` `HLT-001-DEAD-MARKER` `crates/memory-benchmark/src/scorer.rs` - remove or rename the marker, implement the intended behavior, model a typed unsupported state, or move docs/generated/vendor/product-copy text into an allowlisted context
   Route: `Entropy`/`fast`
12. `medium` `HLT-001-DEAD-MARKER` `.` - split large or ambiguous authored code into smaller semantic modules with focused tests
   Route: `Entropy`/`fast`
