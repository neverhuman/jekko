# jankurai Repo Score

- Standard: `jankurai`
- Auditor: `1.6.1`
- Schema: `1.9.0`
- Paper edition: `2026.05-ed8`
- Target stack ID: `rust-ts-vite-react-postgres-bounded-python`
- Target stack: `Rust core + TypeScript/React/Vite + PostgreSQL + generated contracts + exception-only Python AI/data service`
- Repo: `.`
- Run ID: `1781138846`
- Started at: `1781138846`
- Elapsed: `7879` ms
- Scope: `full`
- Raw score: `91`
- Final score: `91`
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

- Status: `review` hard=`0` warning=`44` files=`767`
- Policy: min-lines=`10` min-tokens=`100` max-findings=`50` include-tests=`false` strict=`false`
- Duplicate volume: lines=`129` tokens=`360` bytes=`3618`

- Notes:
  - hard classes are limited to exact active-source file matches and substantial exact same-name units
  - warning classes include same-body different-name units and token/block duplication
  - tests, fixtures, stories, config, Docker, and migrations are omitted unless --include-tests is set

| Kind | Severity | Language | Lines | Tokens | Instances | Reason |
| --- | --- | --- | ---: | ---: | --- | --- |
| `ExactUnitDifferentName` | `Warning` | `rust` | 17 | 54 | `crates/memory-benchmark/src/corpus/real_papers/model.rs:219-236, crates/qbank-builder/src/core_types.rs:80-97` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 8 | `crates/jekko-runtime/src/tool/edit/mod.rs:62-66, crates/jekko-runtime/src/tool/read.rs:55-59, crates/jekko-runtime/src/tool/task.rs:52-56, crates/jekko-runtime/src/tool/webfetch.rs:94-98, crates/jekko-runtime/src/tool/write.rs:48-52` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 12 | `crates/xtask/src/close_issues.rs:127-131, crates/xtask/src/compliance_close.rs:183-187, crates/xtask/src/pr_compliance.rs:146-150, crates/xtask/src/pr_standards.rs:143-147` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 18 | `crates/xtask/src/publish_release.rs:91-97, crates/xtask/src/publish_release_package.rs:183-189, crates/xtask/src/publish_release_registry.rs:229-235` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 10 | 31 | `crates/xtask/src/publish_npm_package.rs:44-54, crates/xtask/src/publish_release_package.rs:142-152` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 9 | 15 | `crates/jankurai-runner/src/bin_main/hero_series/files.rs:25-34, crates/jankurai-runner/src/hero_judge_runner_helpers.rs:132-141` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 7 | 18 | `crates/xtask/src/pr_compliance.rs:83-90, crates/xtask/src/pr_standards.rs:157-164` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 7 | 16 | `crates/jankurai-runner/src/worker_pool.rs:142-149, crates/jankurai-runner/src/worktree.rs:170-177` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 23 | `crates/xtask/src/publish_npm_package.rs:56-62, crates/xtask/src/publish_release_package.rs:154-160` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 6 | 13 | `crates/xtask/src/pr_compliance.rs:75-81, crates/xtask/src/pr_standards.rs:149-155` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 1 | `crates/jankurai-runner/src/hero_judge/population.rs:140-142, crates/jankurai-runner/src/port/plan.rs:161-163, crates/jankurai-runner/src/port_runner/config.rs:65-67, crates/sandboxctl/src/spec_types.rs:169-171` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 1 | `crates/jankurai-runner/src/hero_judge/config.rs:103-105, crates/jankurai-runner/src/hero_judge/population.rs:152-154, crates/jankurai-runner/src/hero_judge/population.rs:156-158, crates/jankurai-runner/src/hero_judge/population.rs:160-162` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 1 | 4 | `crates/jekko-runtime/src/tool/bash.rs:45-46, crates/jekko-runtime/src/tool/edit/mod.rs:58-59, crates/jekko-runtime/src/tool/read.rs:51-52, crates/jekko-runtime/src/tool/task.rs:48-49, crates/jekko-runtime/src/tool/webfetch.rs:90-91, crates/jekko-runtime/src/tool/write.rs:44-45` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 4 | `crates/jekko-runtime/src/tool/edit/mod.rs:105-106, crates/jekko-runtime/src/tool/edit/mod.rs:122-123, crates/jekko-runtime/src/tool/edit/mod.rs:137-138, crates/jekko-runtime/src/tool/read.rs:105-106, crates/jekko-runtime/src/tool/read.rs:122-123, crates/jekko-runtime/src/tool/write.rs:71-72` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 3 | `crates/jekko-tui/benches/scroll_100k.rs:25-26, crates/jekko-tui/benches/scroll_100k.rs:37-38, crates/jekko-tui/benches/scroll_100k.rs:47-48, crates/jekko-tui/benches/scroll_100k.rs:71-72, crates/jekko-tui/benches/scroll_100k.rs:87-88, crates/jekko-tui/benches/scroll_100k.rs:104-105` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 2 | `crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:68-69, crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:89-90, crates/memory-benchmark/src/corpus/real_papers/json_helpers.rs:96-97, crates/zyalc/src/runbook_lint/query.rs:70-71, crates/zyalc/src/runbook_lint/query.rs:125-126, crates/zyalc/src/runbook_lint/query.rs:133-134` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 19 | `crates/xtask/src/close_issues.rs:138-142, crates/xtask/src/compliance_close.rs:194-198` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 18 | `crates/xtask/src/pr_compliance.rs:92-96, crates/xtask/src/pr_management.rs:95-99` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 4 | 12 | `crates/xtask/src/pr_compliance.rs:69-73, crates/xtask/src/pr_standards.rs:137-141` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 3 | 4 | `crates/jekko-tui/src/transcript/syntax/renderer.rs:147-150, crates/jekko-tui/src/transcript/syntax/renderer.rs:166-169` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 7 | `crates/jekko-tui/src/osc52.rs:131-132, crates/jekko-tui/src/osc52.rs:141-142, crates/jekko-tui/src/osc52.rs:149-150` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/xtask/src/commands/jankurai_gate.rs:257-258, crates/xtask/src/commands/jankurai_gate.rs:278-279, crates/xtask/src/commands/jankurai_gate.rs:298-299` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 6 | `crates/qbank-builder/src/cli/discover.rs:224-226, crates/qbank-builder/src/full_text_import_detail_support.rs:161-163` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 3 | `crates/jekko-tui/src/layout/status_pack/tests.rs:61-62, crates/jekko-tui/src/layout/status_pack/tests.rs:119-120, crates/jekko-tui/src/layout/status_pack/tests.rs:130-131` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 5 | `crates/xtask/src/pr_compliance.rs:102-104, crates/xtask/src/pr_standards.rs:172-174` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 2 | 4 | `crates/memory-benchmark/src/corpus/real_papers/model.rs:215-217, crates/qbank-builder/src/core_types.rs:50-52` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 3 | `crates/jekko-core/src/keybind/chord.rs:106-108, crates/jekko-core/src/keybind/set.rs:62-64` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 1 | `crates/zyal-core/src/forbidden.rs:107-108, crates/zyal-core/src/forbidden.rs:115-116, crates/zyal-core/src/forbidden.rs:137-138` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 1 | `crates/xtask/src/pr_workflow_contract/assertions.rs:133-134, crates/xtask/src/pr_workflow_contract/assertions.rs:139-140, crates/xtask/src/pr_workflow_contract/assertions.rs:175-176` | `same body appears under different names across files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 2 | `crates/jankurai-runner/src/hero_judge/population.rs:180-182, crates/jankurai-runner/src/port/target.rs:114-116` | `same-name semantic unit copied across multiple files` |
| `ExactUnitSameName` | `Warning` | `rust` | 2 | 1 | `crates/jankurai-runner/src/hero_judge/population.rs:140-142, crates/jankurai-runner/src/port/plan.rs:161-163` | `same-name semantic unit copied across multiple files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 7 | `crates/jekko-store/src/daemon/port/graph_model.rs:131-132, crates/jekko-store/src/daemon/reasoning/artifacts.rs:161-162` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 7 | `crates/jekko-store/src/daemon/reasoning/artifacts.rs:142-143, crates/jekko-store/src/daemon/reasoning/memory.rs:113-114` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/xtask/src/publish_build_plan.rs:157-158, crates/xtask/src/publish_build_plan.rs:191-192` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/jekko-tui/src/transcript/markup.rs:39-40, crates/zyalc/src/runbook_lint/query.rs:64-65` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 5 | `crates/jekko-provider/src/transform/variants/efforts.rs:65-66, crates/jekko-provider/src/transform/variants/efforts.rs:90-91` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 4 | `crates/qbank-builder/src/paper_tournament/provenance.rs:202-203, crates/qbank-builder/src/paper_tournament/summary.rs:114-115` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 4 | `crates/jekko-tui/src/engine/ansi.rs:176-177, crates/jekko-tui/src/engine/ansi.rs:200-201` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 3 | `crates/jekko-tui/src/transcript/syntax/renderer.rs:189-190, crates/jekko-tui/src/transcript/syntax/renderer.rs:194-195` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 3 | `crates/zyalc/src/compile/validation.rs:218-219, crates/zyalc/src/compile/validation.rs:224-225` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 2 | `crates/jekko-tui/src/anim/glyphs.rs:11-12, crates/jekko-tui/src/anim/glyphs.rs:27-28` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 1 | `crates/jekko-provider/src/transform/variants/efforts.rs:49-50, crates/jekko-provider/src/transform/variants/efforts.rs:78-79` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 0 | `crates/xtask/src/close_stale_prs.rs:231-232, crates/xtask/src/shared.rs:173-174` | `same body appears under different names across files` |
| `ExactUnitDifferentName` | `Warning` | `rust` | 1 | 0 | `crates/jankurai-runner/src/port_runner/helpers.rs:49-49, crates/jekko-tui/src/inline_runtime.rs:101-101` | `same body appears under different names across files` |

## Dimensions

| Dimension | Weight | Score | Weighted | Evidence |
| --- | ---: | ---: | ---: | --- |
| Ownership and navigation surface | 13 | 100 | 13.00 | root `AGENTS.md` present; `CODEOWNERS` present |
| Contract and boundary integrity | 13 | 98 | 12.74 | contract surface found; generated contract artifacts found |
| Proof lanes and test routing | 12 | 100 | 12.00 | one-command setup/validation lane found; deterministic fast lane found |
| Security and supply-chain posture | 12 | 100 | 12.00 | lockfile present; secret or dependency scan tooling found |
| Code shape and semantic surface | 12 | 65 | 7.80 | largest authored code file: crates/xtask/src/commands/split_family.rs (577 LOC); code file exceeds 500 LOC |
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

- Web surface: `true`
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
- Missing CI evidence: `audit-ci, proof-routing, proofbind, proofmark-rust, copy-code, security, ci-bad-behavior, git-bad-behavior, release-bad-behavior, ux-qa, contract-drift, rust-witness, authz-matrix, agent-tool-supply, release-readiness, cost-budget`

| Tool | Category | Mode | Status | Replaced | Artifacts |
| --- | --- | --- | --- | --- | --- |
| `audit-ci` | `audit` | `auto` | `configured` | `manual repo scoring, ad hoc score gates` | `agent/repo-score.json, agent/repo-score.md` |
| `proof-routing` | `proof` | `auto` | `configured` | `ad hoc proof lane selection, manual proof receipts` | `agent/repo-score.json, agent/repo-score.md, target/jankurai/repair-queue.jsonl` |
| `proofbind` | `proof` | `auto` | `ci_evidence` | `manual changed-surface routing, ad hoc proof obligation lists` | `target/jankurai/proofbind/surface-witness.json, target/jankurai/proofbind/obligations.json` |
| `proofmark-rust` | `proof` | `auto` | `configured` | `line-only coverage review, manual in-diff mutation review` | `target/jankurai/proofmark/proofmark-receipt.json, target/jankurai/proofmark/proof-receipt.json` |
| `copy-code` | `audit` | `auto` | `configured` | `ad hoc copy-code review, manual duplication triage` | `target/jankurai/copy-code.json, target/jankurai/copy-code.md` |
| `security` | `security` | `auto` | `configured` | `gitleaks, dependency review, SBOM/provenance` | `target/jankurai/security/evidence.json` |
| `ci-bad-behavior` | `security` | `auto` | `ci_evidence` | `mutable workflow refs, secret echo/debug workflow checks, non-blocking security scans` | `target/jankurai/language-bad-behavior.log` |
| `git-bad-behavior` | `audit` | `auto` | `ci_evidence` | `destructive git automation, force-push release scripts, hidden stash-based state` | `target/jankurai/language-bad-behavior.log` |
| `release-bad-behavior` | `release` | `auto` | `ci_evidence` | `manual release checklist, ad hoc tag and artifact review, manual provenance review` | `target/jankurai/language-bad-behavior.log` |
| `ux-qa` | `ux` | `auto` | `configured` | `playwright, axe-core, visual baselines` | `target/jankurai/ux-qa.json` |
| `db-migration-analyze` | `db` | `auto` | `not_applicable` | `manual migration review` | `target/jankurai/migration-report.json` |
| `contract-drift` | `contract` | `auto` | `configured` | `handwritten contract drift checks, openapi diff` | `agent/repo-score.json, agent/repo-score.md` |
| `rust-witness` | `rust` | `auto` | `ci_evidence` | `manual witness graphing` | `target/jankurai/rust/witness-graph.json` |
| `vibe-coverage` | `audit` | `auto` | `not_applicable` | `manual vibe-coding coverage spreadsheet` | `target/jankurai/vibe-coverage.json, target/jankurai/vibe-coverage.md` |
| `coverage-evidence` | `proof` | `auto` | `not_applicable` | `manual coverage report review, ad hoc mutation survivor review` | `target/jankurai/coverage/coverage-audit.json, target/jankurai/coverage/coverage-audit.md` |
| `authz-matrix` | `security` | `auto` | `configured` | `manual authz matrix review` | `agent/repo-score.json, agent/repo-score.md` |
| `input-boundary` | `security` | `auto` | `not_applicable` | `manual unsafe sink review` | `agent/repo-score.json, agent/repo-score.md` |
| `agent-tool-supply` | `security` | `auto` | `configured` | `manual MCP/tool trust review` | `agent/repo-score.json, agent/repo-score.md` |
| `release-readiness` | `release` | `auto` | `configured` | `manual launch checklist` | `agent/repo-score.json, agent/repo-score.md` |
| `cost-budget` | `release` | `auto` | `configured` | `manual spend review` | `agent/repo-score.json, agent/repo-score.md` |

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
   Fingerprint: `sha256:e37565f20a95ecc9bad748cd75fdd82f73c5e36c97ccee3f0cb9a63317806e34`
   Evidence: largest authored code file: crates/xtask/src/commands/split_family.rs (577 LOC), code file exceeds 500 LOC, most code files stay under 300 LOC, copy-code advisory classes found: 44 (advisory only, no score impact)

## Policy

- Policy file: `./agent/audit-policy.toml`
- Minimum score: `85`
- Fail on: `critical, high`

## Agent Fix Queue

1. `medium` `HLT-001-DEAD-MARKER` `.` - split large or ambiguous authored code into smaller semantic modules with focused tests
   Route: `Entropy`/`fast`
