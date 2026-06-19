#!/usr/bin/env bash
# zyal-serious-live-local.sh - manual-only serious ZYAL live campaign.
#
# Required env:
#   JEKKO_ZYAL_LIVE=1
#   JEKKO_ZYAL_SERIOUS=1
#
# Optional env:
#   SERIOUS_DIR=<path>      override target/zyal/live-serious-<UTC>
#   JEKKO_BIN=<path>        built jekko binary
#   FUSION_SKIP=1           reuse an existing fusion server on 127.0.0.1:4317

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

UTC_STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_ID_PREFIX="${RUN_ID_PREFIX:-serious-${UTC_STAMP}}"
SERIOUS_DIR="${SERIOUS_DIR:-target/zyal/live-serious-${UTC_STAMP}}"
if [[ "$SERIOUS_DIR" = /* ]]; then
  SERIOUS_ABS="$SERIOUS_DIR"
else
  SERIOUS_ABS="$REPO_ROOT/$SERIOUS_DIR"
fi

JEKKO_BIN="${JEKKO_BIN:-$REPO_ROOT/target/release/jekko}"
FUSION_BIN="$REPO_ROOT/jnoccio-fusion/target/debug/jnoccio-fusion"
FUSION_URL="http://127.0.0.1:4317"
USERS_ROOT="${JEKKO_HOME:-$HOME/.jekko}/users"
BALANCER_DB="$USERS_ROOT/.balancer.sqlite"
GLOBAL_EVENTS_PATH="$REPO_ROOT/target/zyal/runner-events.jsonl"

RUN_EXIT=0
VALIDATION_ERRORS=0
MANIFEST_TMP=""

log() { printf '[%s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
fail() { log "ERROR: $*"; exit 1; }

require_opt_in() {
  [[ "${CI:-}" != "true" ]] || fail "CI=true detected; serious live runs are manual-only"
  [[ "${JEKKO_ZYAL_LIVE:-}" = "1" ]] || fail "set JEKKO_ZYAL_LIVE=1"
  [[ "${JEKKO_ZYAL_SERIOUS:-}" = "1" ]] || fail "set JEKKO_ZYAL_SERIOUS=1"
  export JEKKO_KEY_SOURCE_POLICY=users-only
  export JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}"
}

ensure_tools() {
  for cmd in cargo rtk just jq sqlite3 curl tail awk; do
    command -v "$cmd" >/dev/null 2>&1 || fail "missing tool: $cmd"
  done
  if [[ -z "${JEKKO_BIN:-}" ]]; then
    fail "JEKKO_BIN is empty"
  fi
  [[ -x "$JEKKO_BIN" ]] || fail "JEKKO_BIN missing or non-executable: $JEKKO_BIN"
  export JEKKO_BIN
}

snapshot_balancer() {
  local label="$1"
  if [[ -f "$BALANCER_DB" ]]; then
    sqlite3 "$BALANCER_DB" ".dump" > "$SERIOUS_ABS/balancer/${label}.sql" 2>/dev/null \
      || log "WARN: failed to dump balancer at $label"
  fi
}

snapshot_metrics() {
  local label="$1"
  curl -sf --max-time 8 "$FUSION_URL/metrics" \
    > "$SERIOUS_ABS/metrics-snapshots/${label}.prom" 2>/dev/null || true
}

cleanup() {
  log "teardown: killing background PIDs"
  for pidfile in "$SERIOUS_ABS"/pids/*.pid; do
    [[ -f "$pidfile" ]] || continue
    local pid
    pid=$(cat "$pidfile" 2>/dev/null || true)
    [[ -n "$pid" ]] && kill -TERM "$pid" 2>/dev/null || true
  done
  sleep 2
  for pidfile in "$SERIOUS_ABS"/pids/*.pid; do
    [[ -f "$pidfile" ]] || continue
    local pid
    pid=$(cat "$pidfile" 2>/dev/null || true)
    [[ -n "$pid" ]] && kill -KILL "$pid" 2>/dev/null || true
  done
}

start_fusion() {
  if [[ "${FUSION_SKIP:-0}" = "1" ]]; then
    log "FUSION_SKIP=1: using existing fusion server"
    return 0
  fi
  if curl -sf "$FUSION_URL/health" >/dev/null 2>&1; then
    log "fusion already healthy at $FUSION_URL"
    return 0
  fi
  [[ -x "$FUSION_BIN" ]] || fail "fusion binary missing: $FUSION_BIN"
  log "starting jnoccio-fusion in users_pool mode"
  (
    cd "$REPO_ROOT/jnoccio-fusion"
    JNOCCIO_UPSTREAM_KEY_SOURCE=users_pool \
      "$FUSION_BIN" --config config/server.json --env-file .env.jnoccio
  ) > "$SERIOUS_ABS/fusion.log" 2>&1 &
  echo "$!" > "$SERIOUS_ABS/pids/fusion.pid"
}

wait_for_fusion() {
  local elapsed=0
  while [[ $elapsed -lt 60 ]]; do
    if curl -sf "$FUSION_URL/health" >/dev/null 2>&1; then
      log "fusion healthy after ${elapsed}s"
      return 0
    fi
    sleep 3
    elapsed=$((elapsed + 3))
  done
  tail -40 "$SERIOUS_ABS/fusion.log" >&2 2>/dev/null || true
  fail "fusion did not become healthy"
}

assert_fusion_preflight() {
  local health="$SERIOUS_ABS/preflight/fusion-health.json"
  curl -sf "$FUSION_URL/health" > "$health" || fail "fusion /health failed"
  jq -e '
    .upstream_key_source == "users_pool"
    and .user_count == 2
    and ((.per_user_slot_counts.user_1 // 0) > 0)
    and ((.per_user_slot_counts.user_2 // 0) > 0)
  ' "$health" >/dev/null || {
    jq . "$health" >&2 || true
    fail "fusion health is not users_pool with user_1 and user_2"
  }
  snapshot_balancer "preflight"
  snapshot_metrics "preflight"
}

start_observers() {
  touch "$GLOBAL_EVENTS_PATH"
  tail -F -n 0 "$GLOBAL_EVENTS_PATH" > "$SERIOUS_ABS/runner-events.tail.jsonl" 2>/dev/null &
  echo "$!" > "$SERIOUS_ABS/pids/tail.pid"
  (
    while true; do
      snapshot_metrics "metrics-$(date -u +%s)"
      sleep 30
    done
  ) &
  echo "$!" > "$SERIOUS_ABS/pids/metrics.pid"
}

manifest_start() {
  MANIFEST_TMP=$(mktemp)
  echo "[" > "$MANIFEST_TMP"
}

manifest_append() {
  local rid="$1"
  local label="$2"
  local started_at="$3"
  local finished_at="$4"
  local exit_code="$5"
  if [[ "$(wc -l < "$MANIFEST_TMP")" -gt 1 ]]; then
    echo "," >> "$MANIFEST_TMP"
  fi
  jq -n \
    --arg run_id "$rid" \
    --arg label "$label" \
    --argjson started_at "$started_at" \
    --argjson finished_at "$finished_at" \
    --argjson exit_code "$exit_code" \
    '{run_id:$run_id,label:$label,started_at:$started_at,finished_at:$finished_at,duration_s:($finished_at-$started_at),exit_code:$exit_code}' \
    >> "$MANIFEST_TMP"
}

manifest_finish() {
  echo "]" >> "$MANIFEST_TMP"
  jq -n \
    --arg serious_dir "$SERIOUS_DIR" \
    --arg utc_stamp "$UTC_STAMP" \
    --argjson runs "$(cat "$MANIFEST_TMP")" \
    '{schema_version:"zyal.serious_live_manifest.v1", serious_dir:$serious_dir, utc_stamp:$utc_stamp, runs:$runs}' \
    > "$SERIOUS_ABS/manifest.json"
  rm -f "$MANIFEST_TMP"
}

copy_run_artifacts() {
  local rid="$1"
  local src="$REPO_ROOT/target/zyal/runs/$rid"
  local dst="$SERIOUS_ABS/runs/$rid"
  mkdir -p "$dst"
  for name in events.jsonl model_receipts.jsonl summary.json summary.md; do
    if [[ -f "$src/$name" ]]; then
      cp "$src/$name" "$dst/$name"
    fi
  done
}

record_validation_error() {
  local message="$1"
  VALIDATION_ERRORS=$((VALIDATION_ERRORS + 1))
  printf '%s\n' "$message" >> "$SERIOUS_ABS/validation-errors.txt"
  log "VALIDATION: $message"
}

validate_run_artifacts() {
  local rid="$1"
  local require_quality="$2"
  local dir="$REPO_ROOT/target/zyal/runs/$rid"
  local summary="$dir/summary.json"
  local receipts="$dir/model_receipts.jsonl"

  [[ -s "$summary" ]] || { record_validation_error "$rid missing summary.json"; return 0; }
  [[ -s "$receipts" ]] || record_validation_error "$rid missing model_receipts.jsonl"

  jq -e '.budget.max_calls != null' "$summary" >/dev/null \
    || record_validation_error "$rid summary missing budget.max_calls"
  jq -e '(.terminal_status == "run_finished") or (.halt_reason != null)' "$summary" >/dev/null \
    || record_validation_error "$rid summary missing terminal halt reason"
  if [[ "$require_quality" = "1" ]]; then
    jq -e '((.model_calls.by_quality_band.top20 // 0) > 0) or ((.model_calls.by_quality_band.top10 // 0) > 0)' "$summary" >/dev/null \
      || record_validation_error "$rid missing quality-band echo"
  fi
  jq -e 'all(.signals_fired[]?; .id != "empty_response_streak" or (.count // 0) == 0)' "$summary" >/dev/null \
    || record_validation_error "$rid empty-response streak detected"
  jq -e '
    if .model_calls.total_attempts >= 10 then
      (.model_calls.by_user.user_1 // 0) > 0
      and (.model_calls.by_user.user_2 // 0) > 0
      and ([.model_calls.by_user.user_1 // 0, .model_calls.by_user.user_2 // 0] | max) <= (.model_calls.total_attempts * 0.65)
    else
      true
    end
  ' "$summary" >/dev/null || record_validation_error "$rid user balance failed"
}

run_and_capture() {
  local rid="$1"
  local label="$2"
  shift 2
  log "==================== $rid : $label ===================="
  snapshot_balancer "before-$rid"
  snapshot_metrics "before-$rid"
  local started_at finished_at
  started_at=$(date -u +%s)
  /usr/bin/time -v -o "$SERIOUS_ABS/runs/$rid.time" "$@" \
    > "$SERIOUS_ABS/runs/$rid.stdout" \
    2> "$SERIOUS_ABS/runs/$rid.stderr"
  RUN_EXIT=$?
  finished_at=$(date -u +%s)
  snapshot_balancer "after-$rid"
  snapshot_metrics "after-$rid"
  copy_run_artifacts "$rid"
  manifest_append "$rid" "$label" "$started_at" "$finished_at" "$RUN_EXIT"
  log "$rid finished: exit=$RUN_EXIT duration=$((finished_at - started_at))s"
}

write_miniredis_config() {
  jq '
    .worker_cap = 4
    | .advanced_reasoning.worker_cap = 4
    | .live_call_budget.max_calls = 64
    | .live_call_budget.max_parallel = 4
  ' docs/ZYAL/examples/35-rust-redis-replacement-superreasoning.port-run.json \
    > "$SERIOUS_ABS/config/miniredis-serious.port-run.json" \
    || fail "failed to write serious MiniRedis port-run config"
}

run_miniredis_focus() {
  write_miniredis_config
  for idx in 1 2 3 4 5; do
    local rid
    rid=$(printf '%s-miniredis-%02d' "$RUN_ID_PREFIX" "$idx")
    run_and_capture "$rid" "MiniRedis heavy attempt $idx" \
      env JEKKO_REASONING_PARALLEL=1 JEKKO_MODEL_CALL_TIMEOUT_SECS=300 \
      rtk cargo run --manifest-path crates/jekko-runner/Cargo.toml --locked -- \
        --repo . --run-id "$rid" \
        port-run --config "$SERIOUS_ABS/config/miniredis-serious.port-run.json" \
        --live --max-ticks 12
    validate_run_artifacts "$rid" 1
  done
}

run_openqg_focus() {
  local series_id="${RUN_ID_PREFIX}-openqg"
  run_and_capture "$series_id" "OpenQG hero-judge 3-trial series" \
    env JEKKO_MODEL_CALL_TIMEOUT_SECS=300 \
    JEKKO_ZYAL_HERO_MODEL_CALL_BUDGET=64 \
    JEKKO_ZYAL_HERO_MAX_PARALLEL=4 \
    HERO_JUDGE_SERIES_PARALLEL=3 \
    rtk cargo run --manifest-path crates/jekko-runner/Cargo.toml --locked -- \
      --repo . --run-id "$series_id" \
      hero-judge-run --zyal docs/ZYAL/examples/34-superreasoning-openqg-foundry.zyal \
      --live --max-generations 2 --runs 3
  for idx in 1 2 3; do
    local rid
    rid=$(printf '%s-trial-%03d' "$series_id" "$idx")
    copy_run_artifacts "$rid"
    validate_run_artifacts "$rid" 0
  done
}

run_full_ladder() {
  log "starting full 8-rung ladder under $SERIOUS_DIR/full-ladder"
  BATCH_DIR="$SERIOUS_DIR/full-ladder" \
  BATCH_TAG="$RUN_ID_PREFIX" \
  BATCH_RUN_ID_PREFIX="$RUN_ID_PREFIX" \
  BATCH_MINIREDIS_MAX_CALLS=64 \
  BATCH_MINIREDIS_MAX_PARALLEL=4 \
  BATCH_MINIREDIS_MAX_TICKS=12 \
  FUSION_SKIP=1 \
  JEKKO_ZYAL_LIVE=1 \
  JEKKO_ZYAL_SERIOUS=1 \
  JEKKO_BIN="$JEKKO_BIN" \
    scripts/zyal-live-batch.sh
  local ladder_exit=$?
  scripts/zyal-live-report.sh "$SERIOUS_DIR/full-ladder" \
    > "$SERIOUS_ABS/full-ladder-report.stdout" \
    2> "$SERIOUS_ABS/full-ladder-report.stderr"
  local report_exit=$?
  if [[ "$ladder_exit" -ne 0 ]]; then
    record_validation_error "full ladder exited $ladder_exit"
  fi
  if [[ "$report_exit" -ne 0 ]]; then
    record_validation_error "full ladder report exited $report_exit"
  fi
  local ladder_manifest="$SERIOUS_ABS/full-ladder/manifest.json"
  if [[ -s "$ladder_manifest" ]]; then
    local failed
    failed=$(jq -r '[.runs[] | select(.exit_code != 0) | "\(.run_id)=\(.exit_code)"] | join(", ")' "$ladder_manifest")
    if [[ -n "$failed" ]]; then
      record_validation_error "full ladder failed rung(s): $failed"
    fi
  else
    record_validation_error "full ladder missing manifest.json"
  fi
}

write_report() {
  jq -r '
    def row: "| \(.run_id) | \(.label) | \(.duration_s)s | \(.exit_code) |";
    ["| run | label | duration | exit |", "|---|---|---:|---:|"] + (.runs | map(row)) | .[]
  ' "$SERIOUS_ABS/manifest.json" > "$SERIOUS_ABS/report-runs.md"
  {
    printf '# ZYAL serious live report - %s\n\n' "$UTC_STAMP"
    printf '**Artifact root:** `%s`\n\n' "$SERIOUS_DIR"
    printf '**Preflight:** users_pool with `user_1` and `user_2` verified in `preflight/fusion-health.json`.\n\n'
    printf '## Runs\n\n'
    cat "$SERIOUS_ABS/report-runs.md"
    printf '\n## Validation\n\n'
    if [[ "$VALIDATION_ERRORS" -eq 0 ]]; then
      printf 'No local validation errors recorded.\n'
    else
      printf '%s validation error(s):\n\n' "$VALIDATION_ERRORS"
      sed 's/^/- /' "$SERIOUS_ABS/validation-errors.txt"
    fi
    if [[ -f "$SERIOUS_ABS/full-ladder/manifest.json" ]]; then
      printf '\n## Full Ladder\n\n'
      jq -r '
        def row: "| \(.run_id) | \(.label) | \(.duration_s)s | \(.exit_code) |";
        ["| run | label | duration | exit |", "|---|---|---:|---:|"] + (.runs | map(row)) | .[]
      ' "$SERIOUS_ABS/full-ladder/manifest.json"
      if [[ -f "$SERIOUS_ABS/full-ladder/report.md" ]]; then
        printf '\nDetailed ladder report: `%s/full-ladder/report.md`\n' "$SERIOUS_DIR"
      fi
    fi
  } > "$SERIOUS_ABS/report.md"
}

main() {
  require_opt_in
  ensure_tools
  mkdir -p "$SERIOUS_ABS"/{balancer,config,metrics-snapshots,pids,preflight,runs}
  : > "$SERIOUS_ABS/validation-errors.txt"
  trap cleanup EXIT INT TERM
  log "serious dir: $SERIOUS_DIR"
  start_fusion
  wait_for_fusion
  assert_fusion_preflight
  start_observers
  manifest_start
  run_miniredis_focus
  run_openqg_focus
  run_full_ladder
  manifest_finish
  write_report
  log "report: $SERIOUS_DIR/report.md"
  if [[ "$VALIDATION_ERRORS" -ne 0 ]]; then
    fail "serious live validation recorded $VALIDATION_ERRORS error(s)"
  fi
}

main "$@"
