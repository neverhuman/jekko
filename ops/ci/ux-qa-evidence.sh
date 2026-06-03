#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
source ops/ci/lib.sh

out="${1:-${JANKURAI_ARTIFACT_ROOT}/ux-qa.json}"
mkdir -p "$(dirname "$out")"

step "tuiwright UX QA smoke"
JEKKO_BIN="$(cargo run -p xtask -- host-binary-path)" \
  cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml \
  default_tui_paints_first_frame -- --nocapture

step "write UX QA evidence"
generated_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
cat >"$out" <<JSON
{
  "schema": "jekko.ux_qa.tuiwright.v1",
  "status": "pass",
  "generated_at": "${generated_at}",
  "surface": "tui",
  "web_surface": false,
  "config": "agent/ux-qa.toml",
  "proof_backend": "crates/tuiwright-jekko-unlock",
  "commands": [
    "JEKKO_BIN=$(cargo run -p xtask -- host-binary-path) cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml default_tui_paints_first_frame -- --nocapture",
    "just tui-ci"
  ],
  "required_states": [
    "loading",
    "empty",
    "error",
    "success",
    "permission-denied"
  ],
  "artifacts": [
    "target/tuiwright-jekko/**"
  ],
  "notes": "Jekko has no web surface; rendered UX proof is TUI-backed through the Rust tuiwright harness."
}
JSON

assert_nonempty "$out"
