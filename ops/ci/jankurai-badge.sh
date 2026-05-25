#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

source ops/ci/lib.sh

# Refresh the committed README badge artifacts. jankurai audit mutates
# agent/jankurai-badge.{svg,json} and README.md in-process whenever
# agent/badge.toml has `enabled = true`. The score is sourced from the
# committed baseline (agent/baselines/main.repo-score.json), matching the
# upstream neverhuman/jankurai pattern. --json/--md write to a throwaway
# directory so the run leaves no untracked artifacts behind.
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

jankurai audit . \
  --mode ratchet --full \
  --baseline agent/baselines/main.repo-score.json \
  --json "$tmp_dir/repo-score.json" \
  --md "$tmp_dir/repo-score.md" \
  --no-score-history

test -s agent/jankurai-badge.svg
test -s agent/jankurai-badge.json

if [[ "${1:-}" == "--check" ]]; then
  git diff --exit-code -- README.md agent/jankurai-badge.svg agent/jankurai-badge.json
fi
