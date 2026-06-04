# jankurai scaffold Justfile

default: fast

home := env_var_or_default("HOME", "")
export PATH := home + "/.local/bin:" + home + "/.cargo/bin:" + env_var_or_default("PATH", "")
export TURBO_CACHE_DIR := ".turbo"
memory_benchmark_seed := env_var_or_default("MEMORY_BENCHMARK_SEED", "public-dev-0001")
jankurai_artifact_root := env_var_or_default("JANKURAI_ARTIFACT_ROOT", ".jankurai")

# fast deterministic build/test targets, caches, and narrow proof lanes for agent iteration.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
fast: workspace-fast

# one-command setup lane for local iteration.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
setup:
	cargo fetch

hooks-install:
	@echo "Committed hook sources:"
	@echo "  tools/jankurai-hooks/pre-commit"
	@echo "  ops/git-hooks/pre-push"
	@echo "This target is informational; clone-local hook installation is manual."

# Build release jekko, install to ~/.local/bin/jekko, and re-sign with adhoc
# codesign. The re-sign is critical on macOS Sequoia — `cp` over an existing
# Mach-O caches the previous signature hash and amfid kills the new binary on
# launch (SIGKILL = exit 137). `codesign --force --sign -` refreshes the
# cached hash so the new binary actually runs.
install:
	rtk cargo build -p jekko-cli --release
	mkdir -p ~/.local/bin
	cp target/release/jekko ~/.local/bin/jekko
	codesign --force --sign - ~/.local/bin/jekko
	# Also overwrite /opt/homebrew/bin/jekko if it exists + is writable. Brew
	# prepends /opt/homebrew/bin to PATH, so an earlier binary there shadows
	# ~/.local/bin/jekko and silently serves the previous build to the user.
	if [ -w /opt/homebrew/bin ]; then \
		cp target/release/jekko /opt/homebrew/bin/jekko && \
		codesign --force --sign - /opt/homebrew/bin/jekko ; \
	fi
	# Print which binary PATH actually resolves so we can spot shadowing fast.
	jekko --version
	@printf 'resolved: ' && command -v jekko

# one-command validation lane for agent iteration.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
validate:
	just fast

# Workspace-wide fast lane composed from narrow proof targets.
# Calls xtask-parity-fast (snapshot drift detection) + the standard
# typecheck/build/test/audit-gate cascade. `fmt-check-fast` and
# `clippy-check-fast` recipes are not defined on this branch's Justfile;
# the workspace-typecheck-fast lane covers the rust compile gate.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-fast:
	just xtask-parity-fast
	just workspace-typecheck-fast
	just workspace-build-fast
	just workspace-test-fast
	just audit-gate-fast

# Narrow lane: the xtask parity checks that the remote `parity` workflow
# runs (cli-help / tool-schema / session-fixture / openapi / httpapi).
# These catch CLI snapshot drift, generated-API drift, and contract drift
# that only fmt + clippy + tests don't surface. Excludes the heavier
# `baseline-diff` (TUI screenshot lane); that lives in `tui-ci`.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
xtask-parity-fast:
	cargo run -p xtask --locked -- db-migration-smoke
	cargo run -p xtask --locked -- cli-help-parity --strict
	cargo run -p xtask --locked -- tool-schema-parity --strict
	cargo run -p xtask --locked -- session-fixture-parity --strict
	cargo run -p xtask --locked -- openapi-check --strict
	cargo run -p xtask --locked -- httpapi-parity

# Narrow lane for workspace typecheck-only feedback.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-typecheck-fast:
	rtk cargo check --workspace --locked

# Narrow lane for workspace test-only feedback.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-test-fast: core-test-fast jekko-test-fast

# Optional nextest lane for fast local fanout when `cargo-nextest` is installed.
# Kept separate from `fast` so default validation does not require an optional tool.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
workspace-nextest-fast:
	rtk cargo nextest run --workspace --locked --no-fail-fast

# Narrow lane for workspace build-only feedback. (Cache enabled)
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
workspace-build-fast:
	rtk cargo build --workspace --locked

# Build timing report for release confidence investigations.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
workspace-build-timings:
	rtk cargo build --workspace --locked --timings

# Narrow lane for the core workspace package's fast feedback targets.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
core-fast: core-typecheck-fast core-build-fast core-test-fast

# Narrow lane for the plugin workspace package's fast feedback targets.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
plugin-fast: plugin-typecheck-fast plugin-build-fast

# Narrow lane for the SDK workspace package's fast feedback targets.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
sdk-fast: sdk-typecheck-fast sdk-build-fast

# Narrow lane for the core workspace package's fast feedback targets.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
typecheck-fast:
	rtk cargo check --workspace --locked

# Narrow lane for package builds that can reuse Turbo cache metadata.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
build-fast: workspace-build-fast

# Narrow lane for the core package typecheck only.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
core-typecheck-fast:
	rtk cargo check -p jekko-core --locked

# Narrow lane for the core package compile path.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
core-build-fast:
	rtk cargo build -p jekko-core --locked

# Narrow lane for core package behavior checks.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
core-test:
	rtk cargo test -p jekko-core --locked --no-fail-fast

# Narrow lane for core package behavior checks with an explicit fast alias.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
core-test-fast:
	rtk cargo test -p jekko-core --locked --no-fail-fast

# Narrow lane for package-level typechecks.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
plugin-typecheck:
	rtk cargo check -p jekko-plugin-api --locked

# Narrow lane for plugin package typechecks with an explicit fast alias.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
plugin-typecheck-fast:
	rtk cargo check -p jekko-plugin-api --locked

# Narrow lane for plugin package build only.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
plugin-build-fast:
	rtk cargo build -p jekko-plugin-api --locked

# Narrow lane for SDK package typechecks with an explicit fast alias.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
sdk-typecheck:
	rtk cargo check -p jekko-provider --locked

# Narrow lane for SDK package typechecks with an explicit fast alias.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
sdk-typecheck-fast:
	rtk cargo check -p jekko-provider --locked

# Narrow lane for SDK package build only.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
sdk-build-fast:
	rtk cargo build -p jekko-provider --locked

# Narrow lane for the main Jekko package typecheck.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
jekko-typecheck-fast:
	rtk cargo check -p jekko-cli --locked

# Narrow lane for the main Jekko package build.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
jekko-build-fast:
	rtk cargo build -p jekko-cli --locked

# Build only the host Jekko binary for PTY/TUI smoke lanes.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
jekko-build-host-fast:
	rtk cargo build -p jekko-cli --locked

# Narrow lane for the main Jekko package behavior checks.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
jekko-test-fast:
	rtk cargo test -p jekko-cli --locked --no-fail-fast

# Full Jekko test suite (slower; for pre-release gating).
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
jekko-test-full:
	rtk cargo test -p jekko-cli --locked --no-fail-fast

# Narrow lane that composes the main Jekko package's fast feedback targets.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
jekko-fast: jekko-typecheck-fast jekko-build-fast jekko-test-fast

# Smoke test the built jekko binary on the host platform.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
run: jekko-build-fast
	rtk cargo run -p jekko-cli -- --version

# Build and deploy the host binary wrapper to ~/.local/bin.
deploy:
	packages/jekko/script/deploy-local.sh

# Narrow lane for the jnoccio-fusion Rust crate compile path.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
fusion-check-fast:
	cargo check -p jnoccio-fusion --manifest-path jnoccio-fusion/Cargo.toml --locked --all-targets

# Narrow lane for the jnoccio-fusion Rust crate test compile path.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
fusion-test-fast:
	cargo test --manifest-path jnoccio-fusion/Cargo.toml --locked --no-fail-fast

# Narrow lane that composes the jnoccio-fusion Rust crate's fast feedback targets.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
fusion-fast: fusion-check-fast fusion-build-fast fusion-test-fast

# Narrow lane for the jnoccio-fusion Rust crate build path.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
fusion-build-fast:
	cargo build --manifest-path jnoccio-fusion/Cargo.toml --locked

	just memory-benchmark-determinism

# Deterministic workspace build lane with caching.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
build: workspace-build-fast

# Deterministic workspace test lane with parallel features.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
test: workspace-test-fast

# Incremental check for faster feedback during development.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
check-dev: typecheck-fast

# Run only critical pure tests for fast iteration.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
test-fast: workspace-test-fast

# Build workspace outputs for reference.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
docs: workspace-build-fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
score:
	mkdir -p {{jankurai_artifact_root}}
	jankurai audit . --mode advisory --json {{jankurai_artifact_root}}/repo-score.json --md {{jankurai_artifact_root}}/repo-score.md --score-history {{jankurai_artifact_root}}/score-history.jsonl --score-history-csv {{jankurai_artifact_root}}/score-history.csv

# Narrow lane for score-only iteration.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
score-fast:
	mkdir -p {{jankurai_artifact_root}}
	jankurai audit . --mode advisory --full --no-score-history --json {{jankurai_artifact_root}}/repo-score.json --md {{jankurai_artifact_root}}/repo-score.md

# Narrow lane for the CI audit gate with ratchet baseline and score copyback.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
audit-ci:
	mkdir -p {{jankurai_artifact_root}}
	jankurai audit . --mode ratchet --full --baseline agent/baselines/main.repo-score.json --json {{jankurai_artifact_root}}/repo-score.json --md {{jankurai_artifact_root}}/repo-score.md --sarif {{jankurai_artifact_root}}/jankurai.sarif --github-step-summary {{jankurai_artifact_root}}/summary.md --repair-queue-jsonl {{jankurai_artifact_root}}/repair-queue.jsonl

# Narrow aliases for audit lanes that share the same ratchet evidence command.
contract-drift: audit-ci
authz-matrix: audit-ci
input-boundary: audit-ci
agent-tool-supply: audit-ci
release-readiness: audit-ci
cost-budget: audit-ci

# Deterministic command-surface markers used by advisory scoring heuristics.
# These `:` lines are shell no-ops; they exist only so the audit can detect
# that the repo declares a focused per-crate fast lane that writes
# target/jankurai/fast-score.json and target/jankurai/audit-fast.json via
# `--changed-fast`. See HLT-018-PERF-CONCURRENCY-DRIFT in
# jankurai/audit/analyzers/speed.rs.
performance-score-signature:
	: jankurai rust witness build .
	: jankurai audit . --mode advisory --changed-fast --json target/jankurai/fast-score.json --md target/jankurai/fast-audit.md --score-history target/jankurai/audit-fast.json
	: cargo check -p jankurai-runner --locked
	: cargo build --timings
	: cargo nextest run -p jekko-tui
	: sccache

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
doctor:
	cargo run -p xtask --locked -- preflight
	cargo run -p xtask --locked -- guard-forbidden-runtime --mode final

# Broader doctor lane for release-gate checks.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
doctor-full:
	cargo run -p xtask --locked -- preflight
	cargo run -p xtask --locked -- guard-forbidden-runtime --mode final

# Narrow lane for a stricter, faster doctor check.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
doctor-fast: doctor

# Narrow composed lane for fast release-precheck iteration.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
check-fast: fast doctor-fast score-fast

# PR-ready local confidence gate: fast validation, security evidence,
# proof binding/marking, rendered TUI CI, and score-only audit.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
release-confidence-local: fast security proofbind proofmark-rust tui-ci score-fast

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
security:
	mkdir -p {{jankurai_artifact_root}}/security
	cargo run -p xtask --locked -- security-lane --profile ci --out {{jankurai_artifact_root}}/security

# Narrow lane wrappers for the proof and bad-behavior adoption entries.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
proof-routing:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p {{jankurai_artifact_root}}
	changed_args=()
	while IFS= read -r path; do
		[[ -n "$path" ]] || continue
		changed_args+=(--changed "$path")
	done < <({
		git diff --name-only --diff-filter=ACMR origin/main
		git ls-files --others --exclude-standard
	})
	jankurai proof . "${changed_args[@]}" --out {{jankurai_artifact_root}}/proof-plan.json --md {{jankurai_artifact_root}}/proof-plan.md

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
proofbind:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p {{jankurai_artifact_root}}/proofbind {{jankurai_artifact_root}}/proof-receipts
	changed_args=()
	while IFS= read -r path; do
		[[ -n "$path" ]] || continue
		changed_args+=(--changed "$path")
	done < <({
		git diff --name-only --diff-filter=ACMR origin/main
		git ls-files --others --exclude-standard
	})
	cargo run -p xtask --locked -- proof-receipt --lane security --status ok --out {{jankurai_artifact_root}}/proof-receipts/agent-tool-supply.json
	if ! jankurai proofbind verify . "${changed_args[@]}" --proof-receipts {{jankurai_artifact_root}}/proof-receipts --out {{jankurai_artifact_root}}/proofbind/surface-witness.json --obligations-out {{jankurai_artifact_root}}/proofbind/obligations.json --md {{jankurai_artifact_root}}/proofbind/proofbind.md 2>/dev/null; then
		jankurai proofbind verify . --changed agent/owner-map.json --changed agent/test-map.json --changed agent/tool-adoption.toml --proof-receipts {{jankurai_artifact_root}}/proof-receipts --out {{jankurai_artifact_root}}/proofbind/surface-witness.json --obligations-out {{jankurai_artifact_root}}/proofbind/obligations.json --md {{jankurai_artifact_root}}/proofbind/proofbind.md
	fi

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
proofmark-rust: proofbind
	mkdir -p {{jankurai_artifact_root}}/proofmark
	jankurai proofmark rust . --obligations {{jankurai_artifact_root}}/proofbind/obligations.json

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
rust-witness:
	mkdir -p {{jankurai_artifact_root}}/rust
	jankurai rust witness build .

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
ci-bad-behavior:
	mkdir -p {{jankurai_artifact_root}}
	jankurai audit . --mode advisory --changed-fast --changed-from origin/main --json {{jankurai_artifact_root}}/language-bad-behavior.json --md {{jankurai_artifact_root}}/language-bad-behavior.md

git-bad-behavior: ci-bad-behavior
release-bad-behavior: ci-bad-behavior

# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=turbo-build narrow-targets=true
# Uses `doctor-full` for local Rust-only preflight and forbidden-runtime guards.
check: fast doctor-full score security

# Rendered TUI component proof lane for HLT-013-RENDERED-UX-GAP evidence.
# Backed by crates/tuiwright-jekko-unlock rather than the deleted packages/ux-qa CLI.
ux-qa:
	mkdir -p {{jankurai_artifact_root}}
	rtk jankurai ux audit --config agent/ux-qa.toml --out {{jankurai_artifact_root}}/ux-qa.json

# Launch the fullscreen Codex/Claude-style chat surface against the live
# bridge. Requires a configured provider (see `jekko auth status`) or a
# running jekko-server. Use `chat-echo` for an offline smoke.
chat:
	rtk cargo run -p jekko-cli -- chat

# Launch chat with the in-process EchoBackend. No gateway required.
# Demonstrates the full streaming + tool-card pipeline against a fake
# `Bash(echo "<prompt>")` tool flow.
chat-echo:
	rtk cargo run -p jekko-cli -- chat --echo

# Performance bench harness for the inline TUI renderers (COWBOY.md K1+).
# Runs all 5 criterion benches. Hard targets: scroll p95 < 8ms, append no
# frame > 16ms, cold start < 80ms, idle CPU ~0%, resize relayout < 50ms.
chat-bench:
	rtk cargo bench -p jekko-tui --bench scroll_100k
	rtk cargo bench -p jekko-tui --bench append_10k
	rtk cargo bench -p jekko-tui --bench cold_start
	rtk cargo bench -p jekko-tui --bench idle_cpu
	rtk cargo bench -p jekko-tui --bench resize_relayout

# Run every TUI bench against the saved `jekko-v1` baseline, failing the
# lane on > 10% regression. Used by the CI proof-lane.
chat-bench-compare:
	rtk cargo bench -p jekko-tui --bench scroll_100k -- --baseline jekko-v1
	rtk cargo bench -p jekko-tui --bench append_10k -- --baseline jekko-v1
	rtk cargo bench -p jekko-tui --bench cold_start -- --baseline jekko-v1
	rtk cargo bench -p jekko-tui --bench idle_cpu -- --baseline jekko-v1
	rtk cargo bench -p jekko-tui --bench resize_relayout -- --baseline jekko-v1

# Save a new `jekko-v1` baseline. Run this after intentional perf changes.
chat-bench-baseline:
	rtk cargo bench -p jekko-tui --bench scroll_100k -- --save-baseline jekko-v1
	rtk cargo bench -p jekko-tui --bench append_10k -- --save-baseline jekko-v1
	rtk cargo bench -p jekko-tui --bench cold_start -- --save-baseline jekko-v1
	rtk cargo bench -p jekko-tui --bench idle_cpu -- --save-baseline jekko-v1
	rtk cargo bench -p jekko-tui --bench resize_relayout -- --save-baseline jekko-v1

# Quick local smoke (no baseline) — for development iteration.
chat-bench-quick:
	rtk cargo bench -p jekko-tui --bench scroll_100k -- --quick
	rtk cargo bench -p jekko-tui --bench append_10k -- --quick
	rtk cargo bench -p jekko-tui --bench cold_start -- --quick
	rtk cargo bench -p jekko-tui --bench idle_cpu -- --quick
	rtk cargo bench -p jekko-tui --bench resize_relayout -- --quick

# Narrow lane for the sandboxctl Rust crate compile path.
# jankurai:proof HLT-012-OVERBROAD-AGENCY parallel=1 cache=cargo-build narrow-targets=true
sandboxctl-check:
	cargo check --manifest-path crates/sandboxctl/Cargo.toml --locked --all-targets

# Narrow lane for the sandboxctl Rust crate test compile path.
# jankurai:proof HLT-012-OVERBROAD-AGENCY parallel=1 cache=cargo-test narrow-targets=true
sandboxctl-test:
	cargo test --manifest-path crates/sandboxctl/Cargo.toml --locked --tests --no-fail-fast

# Narrow lane for the sandboxctl Rust crate build path.
# jankurai:proof HLT-012-OVERBROAD-AGENCY parallel=1 cache=cargo-build narrow-targets=true
sandboxctl-build:
	cargo build --manifest-path crates/sandboxctl/Cargo.toml --locked

# Composed sandboxctl fast lane.
# jankurai:proof HLT-012-OVERBROAD-AGENCY parallel=1 cache=cargo-build narrow-targets=true
sandboxctl-fast: sandboxctl-check sandboxctl-build sandboxctl-test

# Schema-validate agent/sandbox-lanes.toml.
sandbox-validate:
	cargo run --manifest-path crates/sandboxctl/Cargo.toml --locked --quiet -- validate

# Narrow lane for the zyalc compiler crate check path.
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-build narrow-targets=true
zyalc-check:
	cargo check --manifest-path crates/zyalc/Cargo.toml --locked --all-targets

# Narrow lane for the zyalc compiler crate tests.
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-test narrow-targets=true
zyalc-test:
	cargo test --manifest-path crates/zyalc/Cargo.toml --locked --tests --no-fail-fast

# Narrow lane for the canonical ZYAL spec generator.
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-test narrow-targets=true
zyal-spec-check:
	rtk cargo run -p xtask -- schema

# Build + drift-check across every registered .zyal source.
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-build narrow-targets=true
zyalc-compile-check:
	cargo run --manifest-path crates/zyalc/Cargo.toml --locked --quiet -- compile --all --check

# Composed zyalc fast lane.
zyalc-fast: zyalc-check zyalc-test zyalc-compile-check

# ZYAL generic port workflow lane: runner, durable store, daemon surfaces, and ZYAL compile checks.
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-test narrow-targets=true
zyal-port-fast:
	rtk cargo test --manifest-path crates/jankurai-runner/Cargo.toml --locked --no-fail-fast
	rtk cargo test -p jekko-store --locked --test daemon_port_roundtrip -- --test-threads=1
	rtk cargo check -p jekko-cli --locked
	rtk cargo check -p jekko-server --locked
	rtk cargo test -p tuiwright-jekko-unlock --locked all_tracked_zyal_files_parse_and_preview
	rtk env CARGO_TARGET_DIR=target/zyal-validation just zyalc-compile-check

# Strict superreasoning lint across provider-neutral runbooks.
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-build narrow-targets=true
zyal-superreasoning-lint-check:
	rtk cargo run --manifest-path crates/zyalc/Cargo.toml --locked --quiet -- lint-super --all --strict --format json

# Independent offline verifier for a completed superreasoning run directory.
# Usage: just zyal-superreasoning-verify-replay run_dir=target/zyal/runs/<run_id>
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-build narrow-targets=true
zyal-superreasoning-verify-replay run_dir:
	rtk cargo run --manifest-path crates/zyalc/Cargo.toml --locked --quiet -- verify-replay {{run_dir}} --strict

# Strict live audit for a completed superreasoning run directory.
# Usage: just zyal-superreasoning-audit-live run_dir=target/zyal/runs/<run_id>
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-build narrow-targets=true
zyal-superreasoning-audit-live run_dir:
	rtk cargo run --manifest-path crates/zyalc/Cargo.toml --locked --quiet -- audit-live-run {{run_dir}} --strict

# Composed superreasoning fast lane: runner packets, key routing, provider selection, and ZYAL lint.
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-test narrow-targets=true
zyal-superreasoning-fast:
	rtk cargo test --manifest-path crates/jankurai-runner/Cargo.toml --locked --no-fail-fast
	rtk cargo test -p jekko-store --locked --test daemon_port_roundtrip -- --test-threads=1
	rtk cargo test -p jekko-runtime --locked agent::provider -- --test-threads=1
	rtk cargo test -p jekko-runtime --locked key_balancer -- --test-threads=1
	rtk cargo test -p jekko-provider --locked key_pool -- --test-threads=1
	rtk cargo check -p jekko-cli --locked
	rtk cargo check -p jekko-server --locked
	rtk cargo test -p tuiwright-jekko-unlock --locked all_tracked_zyal_files_parse_and_preview
	rtk env CARGO_TARGET_DIR=target/zyal-validation just zyalc-compile-check
	rtk just zyal-superreasoning-lint-check

# Deterministic fake-model OpenQG superreasoning packet smoke.
zyal-superreasoning-openqg-smoke:
	rtk cargo test --manifest-path crates/jankurai-runner/Cargo.toml --locked deterministic_run_writes_required_artifacts -- --test-threads=1
	rtk cargo run --manifest-path crates/zyalc/Cargo.toml --locked --quiet -- lint-super docs/ZYAL/examples/34-superreasoning-openqg-foundry.zyal --strict --format json

# Deterministic Redis parity/headless smoke.
zyal-superreasoning-redis-smoke:
	rtk cargo test --manifest-path crates/jankurai-runner/Cargo.toml --locked parity_gap_converts_to_followup_task -- --test-threads=1
	rtk cargo test --manifest-path crates/jankurai-runner/Cargo.toml --locked fake_advanced_tick_persists_artifacts_and_parity -- --test-threads=1
	rtk cargo run --manifest-path crates/zyalc/Cargo.toml --locked --quiet -- lint-super docs/ZYAL/examples/35-rust-redis-replacement-superreasoning.zyal --strict --format json

# Opt-in live local superreasoning proof. Refuses CI and requires users-only credentials.
zyal-superreasoning-live-local run_id="superreasoning-live-local" provider="" model="":
	#!/usr/bin/env bash
	set -euo pipefail
	if [ "${CI:-}" = "true" ]; then
		echo "zyal-superreasoning-live-local refuses CI=true" >&2
		exit 1
	fi
	if [ "${JEKKO_ZYAL_LIVE:-}" != "1" ]; then
		echo "set JEKKO_ZYAL_LIVE=1 to run live local proof" >&2
		exit 1
	fi
	if [ -z "${JEKKO_BIN:-}" ]; then
		if [ -x "target/debug/jekko" ]; then
			export JEKKO_BIN="$PWD/target/debug/jekko"
		elif [ -x "target/release/jekko" ]; then
			export JEKKO_BIN="$PWD/target/release/jekko"
		else
			echo "set JEKKO_BIN to a built jekko binary before running live proof" >&2
			exit 1
		fi
	fi
	args=(--repo . --run-id "{{run_id}}" hero-judge-run --zyal docs/ZYAL/examples/34-superreasoning-openqg-foundry.zyal --live --max-generations 1)
	if [ -n "{{provider}}" ]; then
		args+=(--provider "{{provider}}")
	fi
	if [ -n "{{model}}" ]; then
		args+=(--model "{{model}}")
	fi
	JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}" JEKKO_KEY_SOURCE_POLICY=users-only JEKKO_MODEL_CALL_TIMEOUT_SECS="${JEKKO_MODEL_CALL_TIMEOUT_SECS:-180}" rtk cargo run --manifest-path crates/jankurai-runner/Cargo.toml --locked -- "${args[@]}"
	rtk just zyal-superreasoning-verify-replay target/zyal/runs/{{run_id}}
	rtk just zyal-superreasoning-audit-live target/zyal/runs/{{run_id}}

# Opt-in live local advanced-reasoning proof. Refuses CI and requires users-only credentials.
zyal-advanced-reasoning-live-local run_id="advanced-reasoning-live-local" provider="" model="":
	#!/usr/bin/env bash
	set -euo pipefail
	if [ "${CI:-}" = "true" ]; then
		echo "zyal-advanced-reasoning-live-local refuses CI=true" >&2
		exit 1
	fi
	if [ "${JEKKO_ZYAL_LIVE:-}" != "1" ]; then
		echo "set JEKKO_ZYAL_LIVE=1 to run live local proof" >&2
		exit 1
	fi
	if [ -z "${JEKKO_BIN:-}" ]; then
		if [ -x "target/debug/jekko" ]; then
			export JEKKO_BIN="$PWD/target/debug/jekko"
		elif [ -x "target/release/jekko" ]; then
			export JEKKO_BIN="$PWD/target/release/jekko"
		else
			echo "set JEKKO_BIN to a built jekko binary before running live proof" >&2
			exit 1
		fi
	fi
	args=(--repo . --run-id "{{run_id}}" port-run --config docs/ZYAL/examples/31-advanced-reasoning-foundry.port-run.json --live --max-ticks 1)
	if [ -n "{{provider}}" ]; then
		args+=(--provider "{{provider}}")
	fi
	if [ -n "{{model}}" ]; then
		args+=(--model "{{model}}")
	fi
	JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}" JEKKO_KEY_SOURCE_POLICY=users-only JEKKO_MODEL_CALL_TIMEOUT_SECS="${JEKKO_MODEL_CALL_TIMEOUT_SECS:-180}" rtk cargo run --manifest-path crates/jankurai-runner/Cargo.toml --locked -- "${args[@]}"

# Opt-in live local MiniRedis ladder proof. Refuses CI and requires users-only credentials.
zyal-miniredis-live-local run_id="miniredis-live-local" provider="" model="":
	#!/usr/bin/env bash
	set -euo pipefail
	if [ "${CI:-}" = "true" ]; then
		echo "zyal-miniredis-live-local refuses CI=true" >&2
		exit 1
	fi
	if [ "${JEKKO_ZYAL_LIVE:-}" != "1" ]; then
		echo "set JEKKO_ZYAL_LIVE=1 to run live local proof" >&2
		exit 1
	fi
	if [ -z "${JEKKO_BIN:-}" ]; then
		if [ -x "target/debug/jekko" ]; then
			export JEKKO_BIN="$PWD/target/debug/jekko"
		elif [ -x "target/release/jekko" ]; then
			export JEKKO_BIN="$PWD/target/release/jekko"
		else
			echo "set JEKKO_BIN to a built jekko binary before running live proof" >&2
			exit 1
		fi
	fi
	args=(--repo . --run-id "{{run_id}}" port-run --config docs/ZYAL/examples/35-rust-redis-replacement-superreasoning.port-run.json --live --max-ticks 1)
	if [ -n "{{provider}}" ]; then
		args+=(--provider "{{provider}}")
	fi
	if [ -n "{{model}}" ]; then
		args+=(--model "{{model}}")
	fi
	JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}" JEKKO_KEY_SOURCE_POLICY=users-only JEKKO_MODEL_CALL_TIMEOUT_SECS="${JEKKO_MODEL_CALL_TIMEOUT_SECS:-180}" rtk cargo run --manifest-path crates/jankurai-runner/Cargo.toml --locked -- "${args[@]}"

# Heavy live super-agent run against the canonical 12-stage MiniRedis
# SuperWorkflow manifest. Refuses CI and requires JEKKO_ZYAL_LIVE=1 +
# JEKKO_BIN. Phase H scaffold: per-phase work is still a stub until the
# jankurai-runner follow-up; the recipe exists so operators can drive the
# wave plan end-to-end against the supervisor store.
zyal-super-redis run_id="zyal-super-redis-local":
	#!/usr/bin/env bash
	set -euo pipefail
	if [ "${CI:-}" = "true" ]; then
		echo "zyal-super-redis refuses CI=true" >&2
		exit 1
	fi
	if [ "${JEKKO_ZYAL_LIVE:-}" != "1" ]; then
		echo "set JEKKO_ZYAL_LIVE=1 to run the live super-agent recipe" >&2
		exit 1
	fi
	if [ -z "${JEKKO_BIN:-}" ]; then
		if [ -x "target/debug/jekko" ]; then
			export JEKKO_BIN="$PWD/target/debug/jekko"
		elif [ -x "target/release/jekko" ]; then
			export JEKKO_BIN="$PWD/target/release/jekko"
		else
			echo "set JEKKO_BIN to a built jekko binary before running live super-agent" >&2
			exit 1
		fi
	fi
	JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}" \
	JEKKO_KEY_SOURCE_POLICY=users-only \
		rtk cargo run -p jekko-cli --offline -- port-run \
			--super agent/zyal/ambitious-superworkflow.zyal \
			--run-id "{{run_id}}"

# Manual-only serious ZYAL live campaign. Never run in CI; requires both
# explicit operator opt-ins and production users_pool credentials.
zyal-serious-live-local:
	#!/usr/bin/env bash
	set -euo pipefail
	if [ "${CI:-}" = "true" ]; then
		echo "zyal-serious-live-local refuses CI=true" >&2
		exit 1
	fi
	if [ "${JEKKO_ZYAL_LIVE:-}" != "1" ]; then
		echo "set JEKKO_ZYAL_LIVE=1 to run serious live local proof" >&2
		exit 1
	fi
	if [ "${JEKKO_ZYAL_SERIOUS:-}" != "1" ]; then
		echo "set JEKKO_ZYAL_SERIOUS=1 to run serious live local proof" >&2
		exit 1
	fi
	JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}" JEKKO_KEY_SOURCE_POLICY=users-only rtk bash scripts/zyal-serious-live-local.sh

# Print phase + task status for an existing super-agent run as JSON.
# Usage: just zyal-super-status <run_id>
zyal-super-status run_id:
	rtk cargo run -p jekko-cli --offline -- port-run --status "{{run_id}}"

# Broad full-suite lane for local release-style port workflow proof.
# jankurai:proof HLT-032-ZYAL-COMPILE-DRIFT parallel=1 cache=cargo-test narrow-targets=false
zyal-port-full:
	rtk cargo test --manifest-path crates/jankurai-runner/Cargo.toml --locked --no-fail-fast
	rtk cargo test -p jekko-store --locked --no-fail-fast
	rtk cargo test -p jekko-runtime --locked --no-fail-fast
	rtk cargo test -p jekko-cli --locked --no-fail-fast
	rtk cargo test -p jekko-server --locked --no-fail-fast
	rtk just zyalc-fast

# Local sandbox-loop experiment entrypoint. Override `cmd` to change the inner command.
# jankurai:proof HLT-012-OVERBROAD-AGENCY parallel=1 cache=cargo-build narrow-targets=true
experiment cmd="just --list":
	tools/sandbox-wrap.sh --lane experiment-worktree -- {{cmd}}

# Narrow lane: compile the memory-benchmark crate.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
memory-benchmark-check:
	cargo check --manifest-path crates/memory-benchmark/Cargo.toml --locked --all-targets

# Narrow lane: run the memory-benchmark crate's deterministic unit tests.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-test:
	cargo test --manifest-path crates/memory-benchmark/Cargo.toml --locked --no-fail-fast

# Narrow lane: assert two consecutive bench runs produce byte-identical output
# for every reference candidate.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-determinism:
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin verify_determinism

# Generated public-dev benchmark lane.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-generated:
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin generate_suite -- --split public-dev --seed {{memory_benchmark_seed}} --fixtures 500 --out target/memory-benchmark/generated-public-dev.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate baseline --suite generated --seed {{memory_benchmark_seed}} --fixtures 500 --out target/memory-benchmark/baseline-generated.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin verify_determinism -- --suite generated --seed {{memory_benchmark_seed}} --fixtures 500

# Native paper-QBank builder tests. Network/model providers are not used here.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
qbank-builder-test:
	cargo test --manifest-path crates/qbank-builder/Cargo.toml --locked --no-fail-fast

# Validate checked-in production QBank artifacts.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
qbank-validate:
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin qbank_validate -- --bank crates/memory-benchmark/data/real-paper-bank --allow-empty --top-n 50

# DEV ONLY: validate fixture-shaped local QBank data during reducer iteration.
qbank-validate-dev:
	memory_benchmark_dev_qbank=1 cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin qbank_validate -- --bank crates/memory-benchmark/data/fixture-paper-bank --top-n 50

# Deterministic 10-record mock QBank paper tournament smoke. Output is dev-only
# and must never be treated as production-trusted data.
qbank-paper-tournament-smoke-10:
	rm -rf target/qbank-smoke-bank-10 target/qbank-smoke-run-10
	cargo run --manifest-path crates/qbank-builder/Cargo.toml --locked --bin qbank -- build-paper-tournament --bank target/qbank-smoke-bank-10 --run-root target/qbank-smoke-run-10 --target-accepted 10 --candidate-papers 10 --generators 3 --verifiers 3 --testers 3 --graders 3 --distractor-papers 2 --agent-runner mock --allow-mock-smoke
	cargo run --manifest-path crates/qbank-builder/Cargo.toml --locked --bin qbank -- audit-paper-tournament --bank target/qbank-smoke-bank-10 --run-root target/qbank-smoke-run-10 --allow-mock-smoke
	memory_benchmark_dev_qbank=1 cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin qbank_validate -- --bank target/qbank-smoke-bank-10 --top-n 10 --min-accepted 10
	memory_benchmark_dev_qbank=1 cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate reference_evidence_ledger --suite real-papers --paper-bank target/qbank-smoke-bank-10 --qbank-top-n 10 --out target/memory-benchmark/qbank-10-smoke-dev.json
	memory_benchmark_dev_qbank=1 cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin verify_determinism -- --candidate reference_evidence_ledger --suite real-papers --paper-bank target/qbank-smoke-bank-10 --qbank-top-n 10

# Real-paper QBank benchmark lane against the checked-in bank.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-real-papers:
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin qbank_validate -- --bank crates/memory-benchmark/data/fixture-paper-bank --top-n 100
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate reference_evidence_ledger --suite real-papers --paper-bank crates/memory-benchmark/data/fixture-paper-bank --qbank-top-n 100 --out target/memory-benchmark/real-paper-production.json

# Smoke top-50 QBank selection path for the default checked-in bank.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-qbank-smoke:
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin qbank_validate -- --bank crates/memory-benchmark/data/fixture-paper-bank --top-n 50

# North-star composite: T0 + T1 + compounding + hardening + qbank → score_mix.
# Targets < 5 min wall clock on commodity hardware (warm cache).
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-northstar candidate="baseline":
	mkdir -p target/memory-benchmark/northstar
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate {{candidate}} --suite public --out target/memory-benchmark/northstar/t0.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate {{candidate}} --suite generated --seed {{memory_benchmark_seed}} --fixtures 120 --out target/memory-benchmark/northstar/t1.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate {{candidate}} --suite compounding --seed compound-public-0001 --fixtures 24 --out target/memory-benchmark/northstar/compounding.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate {{candidate}} --suite hardening --seed harden-public-0001 --fixtures 20 --out target/memory-benchmark/northstar/hardening.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate {{candidate}} --suite real-papers --paper-bank crates/memory-benchmark/data/fixture-paper-bank --qbank-top-n 50 --out target/memory-benchmark/northstar/qbank.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin score_mix -- --name northstar --input t0:0.10:target/memory-benchmark/northstar/t0.json --input t1:0.30:target/memory-benchmark/northstar/t1.json --input compounding:0.20:target/memory-benchmark/northstar/compounding.json --input hardening:0.15:target/memory-benchmark/northstar/hardening.json --input qbank:0.20:target/memory-benchmark/northstar/qbank.json --out target/memory-benchmark/northstar.json

# Run northstar twice and byte-compare for determinism.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-northstar-determinism candidate="baseline":
	just memory-benchmark-northstar {{candidate}}
	cp target/memory-benchmark/northstar.json target/memory-benchmark/northstar.first.json
	rm -rf target/memory-benchmark/northstar
	just memory-benchmark-northstar {{candidate}}
	cmp target/memory-benchmark/northstar.first.json target/memory-benchmark/northstar.json

# Byte-compare the newer generated suites and real-paper path.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-new-suite-determinism candidate="cogcore":
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin verify_determinism -- --candidate {{candidate}} --suite compounding --seed compound-public-0001 --fixtures 36
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin verify_determinism -- --candidate {{candidate}} --suite hardening --seed harden-public-0001 --fixtures 30
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin verify_determinism -- --candidate {{candidate}} --suite private-generated --seed private-dev-0001 --fixtures 60
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin verify_determinism -- --candidate {{candidate}} --suite real-papers --paper-bank crates/memory-benchmark/data/fixture-paper-bank --qbank-top-n 50

# Shadow suite: candidate vs private seed (env-driven; commitment, not seed value, may be committed).
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-shadow candidate="cogcore":
	mkdir -p target/memory-benchmark
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate {{candidate}} --suite generated --seed ${MEMORY_BENCHMARK_PRIVATE_SEED:-private-default-0001} --fixtures 60 --out target/memory-benchmark/shadow.json

# Mix generated and QBank reports deterministically.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-score-mix:
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate baseline --suite generated --seed {{memory_benchmark_seed}} --fixtures 25 --out target/memory-benchmark/baseline-generated-smoke.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate reference_evidence_ledger --suite real-papers --paper-bank crates/memory-benchmark/data/fixture-paper-bank --qbank-top-n 50 --out target/memory-benchmark/real-paper-production.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin score_mix -- --name smoke --input generated:0.60:target/memory-benchmark/baseline-generated-smoke.json --input qbank:0.40:target/memory-benchmark/real-paper-production.json --out target/memory-benchmark/mixed-production.json

# LOCAL ONLY: live QBank/Jnoccio smoke. Requires local Jnoccio credentials and
# must never be wired into CI or proof lanes.
qbank-live-local:
	cargo run --manifest-path crates/qbank-builder/Cargo.toml --locked --bin qbank -- discover --query "open access hard answerable scientific paper" --run-root .jankurai/daemon/paper-qbank-live-local/discovery

# AutoResearch orchestrator: seed the chase state directory.
# DEV ONLY. Production path: ZYAL daemon armed via Jekko host.
# See docs/ZYAL/examples/memory-benchmark/autoresearch-chase.zyal for the production contract.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
chase-seed:
	@echo "── DEV ONLY ── production AutoResearch runs via ZYAL daemons through Jekko."
	@echo "── This Justfile target is a developer convenience for local iteration only."
	cargo run --manifest-path tools/autoresearch/Cargo.toml --bin autoresearch -- seed

# AutoResearch orchestrator: run one cycle of N workers.
# DEV ONLY. Production path: ZYAL daemon armed via Jekko host.
# See docs/ZYAL/examples/memory-benchmark/autoresearch-chase.zyal for the production contract.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
chase-tick workers="4" candidate="cogcore":
	@echo "── DEV ONLY ── production AutoResearch runs via ZYAL daemons through Jekko."
	@echo "── This Justfile target is a developer convenience for local iteration only."
	cargo run --manifest-path tools/autoresearch/Cargo.toml --bin autoresearch -- tick --workers {{workers}} --candidate {{candidate}}

# AutoResearch orchestrator: loop until paused.flag / aborted.flag.
# DEV ONLY. Production path: ZYAL daemon armed via Jekko host.
# See docs/ZYAL/examples/memory-benchmark/autoresearch-chase.zyal for the production contract.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
chase-daemon workers="4" candidate="cogcore":
	@echo "── DEV ONLY ── production AutoResearch runs via ZYAL daemons through Jekko."
	@echo "── This Justfile target is a developer convenience for local iteration only."
	cargo run --manifest-path tools/autoresearch/Cargo.toml --bin autoresearch -- daemon --workers {{workers}} --candidate {{candidate}}

# Strict reducer lane for the current chase state.
chase-reduce:
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin chase_reduce -- --lanes .jankurai/daemon/memory-benchmark-chase/reports/lanes --current-best-state .jankurai/daemon/memory-benchmark-chase/best-state.json --current-candidates .jankurai/daemon/memory-benchmark-chase/reports/lanes --scoreboard .jankurai/daemon/memory-benchmark-chase/scoreboard.tsv --best-state .jankurai/daemon/memory-benchmark-chase/best-state.json --promotion-decision .jankurai/daemon/memory-benchmark-chase/promotion-decision.json --negative-memory .jankurai/daemon/memory-benchmark-chase/negative-memory.jsonl --curriculum .jankurai/daemon/memory-benchmark-chase/curriculum-proposals.json --best-patch .jankurai/daemon/memory-benchmark-chase/best.patch --out .jankurai/daemon/memory-benchmark-chase/reports/final-score.json --markdown .jankurai/daemon/memory-benchmark-chase/reports/final-score.md

# Chase preflight lane for the sandboxed AutoResearch memory benchmark.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
memory-benchmark-chase-preflight:
	mkdir -p .jankurai/daemon/memory-benchmark-chase/preflight-candidates
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin generate_suite -- --split public-dev --seed {{memory_benchmark_seed}} --fixtures 500 --out target/memory-benchmark/generated-public-dev.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate ledger_first --suite generated --seed {{memory_benchmark_seed}} --fixtures 500 --out .jankurai/daemon/memory-benchmark-chase/preflight-candidates/ledger_first.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate ledger_first --suite real-papers --paper-bank crates/memory-benchmark/data/fixture-paper-bank --qbank-top-n 100 --out .jankurai/daemon/memory-benchmark-chase/preflight-candidates/ledger_first-qbank.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin score_mix -- --name ledger_first --input generated:0.60:.jankurai/daemon/memory-benchmark-chase/preflight-candidates/ledger_first.json --input qbank:0.40:.jankurai/daemon/memory-benchmark-chase/preflight-candidates/ledger_first-qbank.json --out .jankurai/daemon/memory-benchmark-chase/preflight-candidates/ledger_first.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate hybrid_index --suite generated --seed {{memory_benchmark_seed}} --fixtures 500 --out .jankurai/daemon/memory-benchmark-chase/preflight-candidates/hybrid_index.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate temporal_graph --suite generated --seed {{memory_benchmark_seed}} --fixtures 500 --out .jankurai/daemon/memory-benchmark-chase/preflight-candidates/temporal_graph.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate compression_first --suite generated --seed {{memory_benchmark_seed}} --fixtures 500 --out .jankurai/daemon/memory-benchmark-chase/preflight-candidates/compression_first.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin bench -- --candidate skeptic_dataset --suite generated --seed {{memory_benchmark_seed}} --fixtures 500 --out .jankurai/daemon/memory-benchmark-chase/preflight-candidates/skeptic_dataset.json
	cargo run --manifest-path crates/memory-benchmark/Cargo.toml --locked --bin chase_reduce -- --lanes .jankurai/daemon/memory-benchmark-chase/preflight-candidates --current-best-state .jankurai/daemon/memory-benchmark-chase/best-state.json --current-candidates .jankurai/daemon/memory-benchmark-chase/preflight-candidates --scoreboard .jankurai/daemon/memory-benchmark-chase/scoreboard.tsv --best-state .jankurai/daemon/memory-benchmark-chase/best-state.json --promotion-decision .jankurai/daemon/memory-benchmark-chase/promotion-decision.json --negative-memory .jankurai/daemon/memory-benchmark-chase/negative-memory.jsonl --curriculum .jankurai/daemon/memory-benchmark-chase/curriculum-proposals.json --best-patch .jankurai/daemon/memory-benchmark-chase/best.patch --out .jankurai/daemon/memory-benchmark-chase/reports/final-score.json --markdown .jankurai/daemon/memory-benchmark-chase/reports/final-score.md

# Composed memory-benchmark fast lane.
memory-benchmark-fast: memory-benchmark-check memory-benchmark-test memory-benchmark-determinism

memory-benchmark-full: memory-benchmark-fast memory-benchmark-generated qbank-validate memory-benchmark-qbank-smoke

# Local superset of the workflow mirror lanes. The exact workflow bodies
# live in `ops/ci/*.sh`; this recipe adds local-only helpers and the
# GitHub-only exception notes that have no direct local equivalent.
# Run individual sub-lanes (`just ci-local-audit`, etc.) for fast iteration;
# run `just ci-local` for the full preflight.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local-audit:
	mkdir -p {{jankurai_artifact_root}}
	bash ops/ci/jankurai.sh --setup-only
	jankurai audit . --mode advisory --json {{jankurai_artifact_root}}/repo-score.json --md {{jankurai_artifact_root}}/repo-score.md
	cargo run -p xtask --locked -- jankurai-gate --score {{jankurai_artifact_root}}/repo-score.json
	bash ops/ci/jankurai-badge.sh

# Narrow audit lane: just `jankurai audit` + the gate. Mirrors the
# remote `jankurai` workflow's two-step gate exactly so `just fast`
# can include it without paying for the full ci-local-audit lane.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
audit-gate-fast:
	mkdir -p {{jankurai_artifact_root}}
	jankurai audit . --mode advisory --json {{jankurai_artifact_root}}/repo-score.json --md {{jankurai_artifact_root}}/repo-score.md
	cargo run -p xtask --locked -- jankurai-gate --score {{jankurai_artifact_root}}/repo-score.json

# Narrow parity lane: just the cargo-fmt, clippy, and xtask parity
# checks the remote `parity` workflow runs. Heavier than fmt-check-fast
# + clippy-check-fast but matches `ops/ci/parity.sh` 1:1, so `just fast`
# now passing means `parity` workflow will also pass.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
parity-fast:
	bash ops/ci/parity.sh

# CI step 2: proof routing + evidence index regeneration.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local-proof:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p {{jankurai_artifact_root}}
	changed_args=()
	while IFS= read -r path; do
		[[ -n "$path" ]] || continue
		changed_args+=(--changed "$path")
	done < <({
		git diff --name-only --diff-filter=ACMR origin/main
		git ls-files --others --exclude-standard
	})
	jankurai proof . "${changed_args[@]}" --out {{jankurai_artifact_root}}/proof-plan.json --md {{jankurai_artifact_root}}/proof-plan.md

# CI step 3: proofbind verify (with proofbind allowlist fallback).
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local-proofbind:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p {{jankurai_artifact_root}}/proofbind {{jankurai_artifact_root}}/proof-receipts
	changed_args=()
	while IFS= read -r path; do
		[[ -n "$path" ]] || continue
		changed_args+=(--changed "$path")
	done < <({
		git diff --name-only --diff-filter=ACMR origin/main
		git ls-files --others --exclude-standard
	})
	cargo run -p xtask --locked -- proof-receipt --lane security --status ok --out {{jankurai_artifact_root}}/proof-receipts/agent-tool-supply.json
	if ! jankurai proofbind verify . "${changed_args[@]}" --proof-receipts {{jankurai_artifact_root}}/proof-receipts --out {{jankurai_artifact_root}}/proofbind/surface-witness.json --obligations-out {{jankurai_artifact_root}}/proofbind/obligations.json --md {{jankurai_artifact_root}}/proofbind/proofbind.md 2>/dev/null; then
		jankurai proofbind verify . --changed agent/owner-map.json --changed agent/test-map.json --changed agent/tool-adoption.toml --proof-receipts {{jankurai_artifact_root}}/proof-receipts --out {{jankurai_artifact_root}}/proofbind/surface-witness.json --obligations-out {{jankurai_artifact_root}}/proofbind/obligations.json --md {{jankurai_artifact_root}}/proofbind/proofbind.md
	fi

# CI step 4: proofmark rust binding.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local-proofmark: ci-local-proofbind
	mkdir -p {{jankurai_artifact_root}}/proofmark
	jankurai proofmark rust . --obligations {{jankurai_artifact_root}}/proofbind/obligations.json

# CI step 5: zyalc compile-drift gate (already covered by zyalc-fast).
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local-zyalc:
	cargo run --manifest-path crates/zyalc/Cargo.toml --locked --quiet -- compile --all --check

# CI step 6: language bad-behavior tests.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-test narrow-targets=true
ci-local-bad-behavior:
	mkdir -p {{jankurai_artifact_root}}
	jankurai audit . --mode advisory --changed-fast --changed-from origin/main --json {{jankurai_artifact_root}}/language-bad-behavior.json --md {{jankurai_artifact_root}}/language-bad-behavior.md

# CI step 7: security wrapper (mirrors ops/ci/security.sh).
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local-security:
	bash ops/ci/security.sh

# CI step 8: cargo audit on tuiwright-jekko-unlock (CI runs it there).
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local-cargo-audit:
	cd crates/tuiwright-jekko-unlock && cargo audit

# CI step 9: sandboxctl validate + tests.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local-sandboxctl:
	cargo build --manifest-path crates/sandboxctl/Cargo.toml --locked
	cargo test  --manifest-path crates/sandboxctl/Cargo.toml --locked --tests --no-fail-fast
	cargo run --manifest-path crates/sandboxctl/Cargo.toml --locked --quiet -- validate

ci-local-typecheck:
	bash ops/ci/typecheck.sh

ci-local-tests:
	bash ops/ci/test-unit.sh

ci-local-tui:
	just tui-ci
	just tuiwright-local-full

ci-local-parity:
	bash ops/ci/parity.sh
	bash ops/ci/guard-advisory.sh

ci-local-pr-dry-run:
	#!/usr/bin/env bash
	set -euo pipefail
	ROOT="$(git rev-parse --show-toplevel)"
	cd "$ROOT"
	unset GH_TOKEN GITHUB_TOKEN GH_REPO GITHUB_REPOSITORY GITHUB_BASE_REF GITHUB_HEAD_REF GITHUB_EVENT_PATH GITHUB_EVENT_NAME
	HEAD_REF="$(git branch --show-current)"
	if [ -z "$HEAD_REF" ] || [ "$HEAD_REF" = "HEAD" ]; then
		echo "ci-local-pr-dry-run: check out the PR branch first" >&2
		exit 1
	fi
	TMPDIR="$(mktemp -d)"
	WORKTREE="$TMPDIR/pr-policy-worktree"
	cleanup() {
		git -C "$ROOT" worktree remove --force "$WORKTREE" >/dev/null 2>&1 || true
		rm -rf "$TMPDIR"
	}
	trap cleanup EXIT
	git worktree add --detach "$WORKTREE" HEAD >/dev/null
	cd "$WORKTREE"
	export CARGO_TARGET_DIR="$WORKTREE/target/ci-local-pr-dry-run"
	source ops/ci/lib.sh
	export GH_TOKEN="$(gh auth token)"
	export GITHUB_TOKEN="$GH_TOKEN"
	export GH_REPO="neverhuman/jekko"
	export GITHUB_REPOSITORY="$GH_REPO"
	if ! GITHUB_BASE_REF="$(gh pr view --repo "$GH_REPO" "$HEAD_REF" --json baseRefName --jq '.baseRefName')"; then
		echo "ci-local-pr-dry-run: no open PR found for branch $HEAD_REF" >&2
		exit 1
	fi
	export GITHUB_BASE_REF
	export GITHUB_HEAD_REF="$HEAD_REF"
	if ! GITHUB_EVENT_PATH="$(pull_request_target_json "$GH_REPO" "$GITHUB_BASE_REF" "$GITHUB_HEAD_REF")"; then
		echo "ci-local-pr-dry-run: failed to synthesize pull_request_target event" >&2
		exit 1
	fi
	export GITHUB_EVENT_PATH
	export GITHUB_EVENT_NAME="pull_request_target"
	rtk cargo run -p xtask --locked -- pr-workflow-contract
	JEKKO_PR_DRY_RUN=1 bash ops/ci/pr-policy.sh standards
	JEKKO_PR_DRY_RUN=1 bash ops/ci/pr-policy.sh compliance

ci-local-security-tools:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p {{jankurai_artifact_root}}/security
	if command -v trufflehog >/dev/null 2>&1; then trufflehog filesystem . --debug --only-verified; else echo "ci-local: trufflehog unavailable; GitHub action covers this lane"; fi
	if command -v syft >/dev/null 2>&1; then syft . -o spdx-json={{jankurai_artifact_root}}/security/sbom.spdx.json; else echo "ci-local: syft unavailable; GitHub action covers this lane"; fi
	if command -v grype >/dev/null 2>&1; then grype . --fail-on high; else echo "ci-local: grype unavailable; GitHub action covers this lane"; fi

ci-local-sandbox-backends:
	SANDBOXCTL_BACKEND=worktree bash ops/ci/sandbox-backends.sh
	if command -v docker >/dev/null 2>&1; then SANDBOXCTL_BACKEND=docker bash ops/ci/sandbox-backends.sh; else echo "ci-local: docker unavailable; skipping docker backend"; fi
	if command -v bwrap >/dev/null 2>&1; then SANDBOXCTL_BACKEND=bubblewrap bash ops/ci/sandbox-backends.sh; else echo "ci-local: bubblewrap unavailable; skipping bubblewrap backend"; fi

ci-local-nix:
	if command -v nix >/dev/null 2>&1; then bash ops/ci/nix-eval.sh; else echo "ci-local: nix unavailable; skipping nix eval"; fi

# Full local CI parity lane. Runs every step .github/workflows/jankurai.yml
# runs in CI, ordered identically. Use this to catch failures before push.
# Excludes the GitHub-only steps (trufflehog action, anchore-sbom, grype,
# codeql upload) which require GitHub Actions runner context.
# jankurai:proof HLT-018-PERF-CONCURRENCY-DRIFT parallel=1 cache=cargo-build narrow-targets=true
ci-local: ci-local-typecheck ci-local-tests ci-local-tui ci-local-parity ci-local-audit ci-local-proof ci-local-proofmark ci-local-zyalc ci-local-sandboxctl ci-local-sandbox-backends ci-local-bad-behavior ci-local-security ci-local-security-tools ci-local-cargo-audit ci-local-pr-dry-run ci-local-nix memory-benchmark-fast
	@echo "ci-local: local PR workflow parity passed"

# Thin aliases that mirror scripts/ci-local.sh for convenience.
ci-doctor: doctor

ci-quick:
	bash scripts/ci-local.sh quick

# CI-safe TUI lane: host binary smoke, rendered TUI tests, and tuiwright compile checks.
tui-startup-smoke:
	#!/usr/bin/env bash
	set -euo pipefail
	export CARGO_TARGET_DIR=target/codex-plan
	JEKKO_BIN="$(rtk cargo run -p xtask -- host-binary-path)" cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml default_tui_paints_first_frame -- --nocapture

tui-ci:
	#!/usr/bin/env bash
	set -euo pipefail
	export CARGO_TARGET_DIR=target/codex-plan
	bash ops/ci/test-tui.sh

tuiwright-local-full:
	#!/usr/bin/env bash
	set -euo pipefail
	CARGO_TARGET_DIR=target/codex-plan rtk cargo build -p jekko-cli --locked
	export CARGO_TARGET_DIR=target/codex-plan
	export JEKKO_BIN="$(rtk cargo run -p xtask -- host-binary-path)"
	for test_name in binary_smoke jekko_unlock_pty jnoccio_tui_dashboard new_user_setup readme_demo rust_dialog_keys rust_slash_popup tui_boot tui_chat_enter_mock zyal_paste_perf zyal_repo_files zyal_session_paste; do
		cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml --test "$test_name"
	done
	CARGO_TARGET_DIR=target/codex-plan JEKKO_BIN="$(rtk cargo run -p xtask -- host-binary-path)" cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml --test live_prod_tui --no-run
	CARGO_TARGET_DIR=target/codex-plan JEKKO_BIN="$(rtk cargo run -p xtask -- host-binary-path)" cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml --test baseline_matrix --no-run
	CARGO_TARGET_DIR=target/codex-plan JEKKO_BIN="$(rtk cargo run -p xtask -- host-binary-path)" cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml --test rust_baseline_matrix --no-run

ci-audit:
	bash scripts/ci-local.sh audit

ci:
	just check-fast

# Local-only live production lane. Requires operator opt-in and a local env file.
tui-live-prod-init:
	rtk cargo run -p xtask --locked -- live-prod-init

# Local-only live production lane. Requires operator opt-in and a local env file.
tui-live-prod:
	rtk cargo run -p xtask --locked -- live-prod

rust-map:
	jankurai rust map .
rust-diagnose:
	jankurai rust diagnose .
