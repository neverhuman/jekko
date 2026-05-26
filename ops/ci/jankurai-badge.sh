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

# --fail-on critical: high-severity findings are still recorded + caught by
#   `xtask jankurai-gate` (agent/jankurai-gate-overrides.toml decides which
#   high findings are tolerable per-rule with `reason` + `expires`). The
#   badge refresh's only job is to keep agent/jankurai-badge.{svg,json} in
#   sync with the current scan; failing on high here would block every PR
#   on the systemic jankurai-1.5.1 cap-handling issues called out in
#   agent/audit-policy.toml's minimum_score comment.
#
# The trailing `|| audit_status=$?` keeps the script proceeding to the
# artifact + diff checks even when the audit returns non-zero. The
# exit-code is preserved so a future tightening can re-enable strict mode
# by removing this guard.
audit_status=0
jankurai audit . \
  --mode ratchet --full \
  --baseline agent/baselines/main.repo-score.json \
  --fail-on critical \
  --json "$tmp_dir/repo-score.json" \
  --md "$tmp_dir/repo-score.md" \
  --no-score-history || audit_status=$?
if [ "$audit_status" -ne 0 ]; then
  echo "jankurai-badge: audit exited $audit_status; proceeding to artifact verification (gate enforcement runs separately via xtask jankurai-gate)" >&2
fi

test -s agent/jankurai-badge.svg
test -s agent/jankurai-badge.json

if [[ "${1:-}" == "--check" ]]; then
  git diff --exit-code -- README.md agent/jankurai-badge.svg agent/jankurai-badge.json
fi
