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

# This is a badge-SYNC step, not a quality gate. The authoritative score gate
# is the separate `cargo run -p xtask -- jankurai-gate` step, which honours the
# waivers in agent/jankurai-gate-overrides.toml. `jankurai audit --mode ratchet`
# returns a non-zero exit when the live full-repo score dips below the policy
# floor (agent/audit-policy.toml minimum_score); that floor is enforced by the
# gate step, so we must NOT let it abort the badge refresh too — otherwise a
# transient repo-wide score dip blocks every PR (including ones that pass the
# real gate via overrides). Tolerate the ratchet exit here; the genuine
# badge-drift check below (`git diff --exit-code`) still runs unconditionally.
audit_status=0
jankurai audit . \
  --mode ratchet --full \
  --baseline agent/baselines/main.repo-score.json \
  --json "$tmp_dir/repo-score.json" \
  --md "$tmp_dir/repo-score.md" \
  --no-score-history || audit_status=$?
if [[ "$audit_status" -ne 0 ]]; then
  echo "jankurai-badge: audit returned ${audit_status} (below floor); badge refresh continues — score floor is gated by the jankurai-gate step." >&2
fi

test -s agent/jankurai-badge.svg
test -s agent/jankurai-badge.json

if [[ "${1:-}" == "--check" ]]; then
  git diff --exit-code -- README.md agent/jankurai-badge.svg agent/jankurai-badge.json
fi
