#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

source ops/ci/lib.sh

# Refresh the committed README badge artifacts. The score is sourced from the
# committed baseline (agent/baselines/main.repo-score.json), matching the
# upstream neverhuman/jankurai pattern. The actual audit gate runs separately in
# ops/ci/jankurai.sh plus the override-aware xtask jankurai-gate step.

if [[ "${1:-}" == "--check" ]]; then
  jankurai badge . \
    --score agent/baselines/main.repo-score.json \
    --out agent/jankurai-badge.svg \
    --json-out agent/jankurai-badge.json \
    --readme README.md \
    --link agent/baselines/main.repo-score.json \
    --update-readme
  git diff --exit-code -- README.md agent/jankurai-badge.svg agent/jankurai-badge.json
  test -s agent/jankurai-badge.svg
  test -s agent/jankurai-badge.json
  exit 0
fi

if [[ "${1:-}" != "--check" ]]; then
  jankurai badge . \
    --score agent/baselines/main.repo-score.json \
    --out agent/jankurai-badge.svg \
    --json-out agent/jankurai-badge.json \
    --readme README.md \
    --link agent/baselines/main.repo-score.json \
    --update-readme
fi

test -s agent/jankurai-badge.svg
test -s agent/jankurai-badge.json
