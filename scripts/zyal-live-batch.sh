#!/usr/bin/env bash
# zyal-live-batch.sh — drive the 8-run ZYAL live-LLM stress ladder
#
# Companion to scripts/zyal-live-report.sh. Produces a single timestamped
# batch directory under target/zyal/live-batch-<UTC>/ that the reporter folds
# into a mermaid-rich report.md.
#
# Required env:
#   JEKKO_ZYAL_LIVE=1        (the live recipes refuse to run without this)
#   JEKKO_BIN=<path>         (built jekko binary; defaults to target/release/jekko)
#
# Optional env:
#   BATCH_TAG=<slug>         (appended to the batch directory name)
#   BATCH_DIR=<path>         (override the output directory)
#   BATCH_SKIP_SMOKE=1       (skip r0 cargo test)
#   BATCH_ONLY="r1 r2 r5"    (whitespace-separated; restrict which runs execute)
#   FUSION_SKIP=1            (do not launch jnoccio-fusion — useful if you
#                            already have one bound to 127.0.0.1:4317)

set -uo pipefail

# ---------- paths + globals ----------------------------------------------

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

UTC_STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BATCH_TAG="${BATCH_TAG:-toasty-toucan}"
BATCH_DIR="${BATCH_DIR:-target/zyal/live-batch-${UTC_STAMP}-${BATCH_TAG}}"
BATCH_ABS="$REPO_ROOT/$BATCH_DIR"

JEKKO_BIN="${JEKKO_BIN:-$REPO_ROOT/target/release/jekko}"
FUSION_BIN="$REPO_ROOT/jnoccio-fusion/target/debug/jnoccio-fusion"
FUSION_URL="http://127.0.0.1:4317"

USERS_ROOT="${JEKKO_HOME:-$HOME/.jekko}/users"
BALANCER_DB="$USERS_ROOT/.balancer.sqlite"
SUPER_DB="${JEKKO_HOME:-$HOME/.jekko}/zyal-supervisor.sqlite"

GLOBAL_EVENTS_PATH="$REPO_ROOT/target/zyal/runner-events.jsonl"

# Recipes ordered r0..r7 -- see plan i-need-you-to-toasty-toucan.md §The 8-run ladder
declare -a RUN_IDS=(
  "r0-smoke"
  "advanced-r1-serial"
  "advanced-r2-parallel"
  "super-r3-serial"
  "super-r4-parallel"
  "miniredis-r5"
  "super-redis-r6-planwalk"
  "super-r7-patient"
)
declare -a RUN_LABELS=(
  "cargo test --workspace (no-LLM smoke)"
  "zyal-advanced-reasoning-live-local advanced-r1-serial (serial)"
  "zyal-advanced-reasoning-live-local advanced-r2-parallel (parallel)"
  "zyal-superreasoning-live-local super-r3-serial (serial)"
  "zyal-superreasoning-live-local super-r4-parallel (parallel)"
  "zyal-miniredis-live-local miniredis-r5 (parallel)"
  "zyal-super-redis super-redis-r6-planwalk (plan-walk, no LLM)"
  "zyal-superreasoning-live-local super-r7-patient (300s timeout, parallel)"
)

if [[ -n "${BATCH_RUN_ID_PREFIX:-}" ]]; then
  for idx in "${!RUN_IDS[@]}"; do
    RUN_IDS[$idx]="${BATCH_RUN_ID_PREFIX}-${RUN_IDS[$idx]}"
  done
fi

# ---------- helpers ------------------------------------------------------

log() { printf '[%s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }
fail() { log "ERROR: $*"; exit 1; }

# Returns the per-run events.jsonl path. The recipes drive jekko-runner /
# jekko-cli with --run-id <RUN_IDS[i]>, so files live at
# target/zyal/runs/<run_id>/events.jsonl.
events_path_for() {
  local rid="$1"
  echo "$REPO_ROOT/target/zyal/runs/$rid/events.jsonl"
}

snapshot_balancer() {
  local label="$1"
  if [[ -f "$BALANCER_DB" ]]; then
    sqlite3 "$BALANCER_DB" ".dump" > "$BATCH_ABS/balancer/${label}.sql" 2>/dev/null \
      || log "WARN: failed to dump balancer at $label"
  fi
}

snapshot_super_store() {
  local label="$1"
  if [[ -f "$SUPER_DB" ]]; then
    sqlite3 "$SUPER_DB" \
      "SELECT run_id, workflow_id, status, created_at, updated_at FROM zyal_super_runs ORDER BY created_at DESC LIMIT 20" \
      > "$BATCH_ABS/super/${label}.tsv" 2>/dev/null \
      || log "WARN: failed to dump zyal_super_runs at $label"
  fi
}

# ---------- pre-flight ---------------------------------------------------

log "batch dir: $BATCH_DIR"
mkdir -p \
  "$BATCH_ABS"/{config,runs,metrics-snapshots,balancer,super,pids}

[[ -x "$JEKKO_BIN" ]] || fail "JEKKO_BIN missing or non-executable: $JEKKO_BIN"
export JEKKO_BIN
log "JEKKO_BIN=$JEKKO_BIN"

if [[ "${CI:-}" = "true" ]]; then
  fail "CI=true detected — the live recipes refuse to run in CI"
fi

if [[ "${JEKKO_ZYAL_LIVE:-}" != "1" ]]; then
  log "WARN: JEKKO_ZYAL_LIVE!=1 — the live recipes will refuse to run; only r0 + r6 will work"
fi

for cmd in cargo rtk just jq sqlite3 curl tail awk; do
  command -v "$cmd" >/dev/null 2>&1 || fail "missing tool: $cmd"
done

# Verify multi-user pool
slot_count=0
for slot in user user_1 user_2; do
  if [[ -f "$USERS_ROOT/$slot/llm.env" ]]; then
    n=$(grep -cE '^[A-Z][A-Z0-9_]+=' "$USERS_ROOT/$slot/llm.env" 2>/dev/null || echo 0)
    log "users/$slot/llm.env: $n env vars"
    [[ "$n" -gt 0 ]] && slot_count=$((slot_count+1))
  else
    log "users/$slot/llm.env: missing"
  fi
done
log "active key slots: $slot_count"
[[ "$slot_count" -ge 1 ]] || fail "no active key slots — populate ~/.jekko/users/*/llm.env"

# ---------- jnoccio-fusion background ------------------------------------

FUSION_PID=""
TAIL_PID=""
METRICS_PID=""

cleanup() {
  log "teardown: killing background PIDs"
  for pidfile in "$BATCH_ABS"/pids/*.pid; do
    [[ -f "$pidfile" ]] || continue
    local pid
    pid=$(cat "$pidfile" 2>/dev/null || true)
    [[ -n "$pid" ]] && kill -TERM "$pid" 2>/dev/null || true
  done
  sleep 2
  for pidfile in "$BATCH_ABS"/pids/*.pid; do
    [[ -f "$pidfile" ]] || continue
    local pid
    pid=$(cat "$pidfile" 2>/dev/null || true)
    [[ -n "$pid" ]] && kill -KILL "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT INT TERM

start_fusion() {
  if [[ "${FUSION_SKIP:-0}" = "1" ]]; then
    log "FUSION_SKIP=1 → not launching jnoccio-fusion"
    return 0
  fi
  if ss -ltn 2>/dev/null | grep -q '127.0.0.1:4317'; then
    log "port 4317 already bound → assuming fusion is up"
    return 0
  fi
  [[ -x "$FUSION_BIN" ]] || fail "fusion binary missing: $FUSION_BIN (run: cargo build -p jnoccio-fusion --manifest-path jnoccio-fusion/Cargo.toml)"
  log "starting jnoccio-fusion → $BATCH_DIR/fusion.log (key_source=${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool})"
  (
    cd "$REPO_ROOT/jnoccio-fusion"
    # The ladder is built around the users_pool fan-out across
    # ~/.jekko/users/<id>/llm.env. Without this env var fusion defaults
    # to legacy config_env (single .env.jnoccio pool) — which silently
    # bypasses the multi-tenant credential design the recipes assume.
    JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}" \
      "$FUSION_BIN" --config config/server.json --env-file .env.jnoccio
  ) > "$BATCH_ABS/fusion.log" 2>&1 &
  FUSION_PID=$!
  echo "$FUSION_PID" > "$BATCH_ABS/pids/fusion.pid"
  log "fusion PID=$FUSION_PID"

  # Wait for /health
  local elapsed=0
  while [[ $elapsed -lt 60 ]]; do
    if curl -sf "$FUSION_URL/health" >/dev/null 2>&1; then
      log "fusion healthy after ${elapsed}s"
      return 0
    fi
    sleep 3
    elapsed=$((elapsed+3))
    if ! kill -0 "$FUSION_PID" 2>/dev/null; then
      log "fusion died during boot; tail of fusion.log:"
      tail -20 "$BATCH_ABS/fusion.log" >&2 || true
      fail "fusion failed to start"
    fi
  done
  log "WARN: fusion did not pass /health in 60s; continuing anyway"
}

start_observers() {
  # global event tail (starts before any runs to capture warm-up traffic)
  touch "$GLOBAL_EVENTS_PATH"
  tail -F -n 0 "$GLOBAL_EVENTS_PATH" > "$BATCH_ABS/runner-events.tail.jsonl" 2>/dev/null &
  TAIL_PID=$!
  echo "$TAIL_PID" > "$BATCH_ABS/pids/tail.pid"
  log "global event tail PID=$TAIL_PID"

  # /metrics scraper every 30s
  (
    while true; do
      ts=$(date -u +%s)
      curl -sf --max-time 8 "$FUSION_URL/metrics" \
        > "$BATCH_ABS/metrics-snapshots/metrics-${ts}.prom" 2>/dev/null \
        || true
      sleep 30
    done
  ) &
  METRICS_PID=$!
  echo "$METRICS_PID" > "$BATCH_ABS/pids/metrics.pid"
  log "metrics scraper PID=$METRICS_PID"
}

# ---------- recipe drivers ----------------------------------------------

# Each driver returns the exit code in $RUN_EXIT.
RUN_EXIT=0
BATCH_FAILURES=0
declare -a BATCH_FAILED_RUNS=()

base_run_id() {
  local rid="$1"
  if [[ -n "${BATCH_RUN_ID_PREFIX:-}" && "$rid" == "$BATCH_RUN_ID_PREFIX-"* ]]; then
    rid="${rid#"$BATCH_RUN_ID_PREFIX-"}"
  fi
  echo "$rid"
}

run_r0() {
  local rid="$1"
  log "$rid: cargo test --workspace --locked --no-fail-fast"
  # The tuiwright baseline_matrix capture suite now requires explicit
  # JEKKO_TUI_CAPTURE=1 (added in the GOD-logging session), so it skips
  # silently here even with JEKKO_BIN set globally for the live recipes.
  /usr/bin/time -v -o "$BATCH_ABS/runs/$rid.time" \
    cargo test --workspace --locked --no-fail-fast \
      > "$BATCH_ABS/runs/$rid.stdout" \
      2> "$BATCH_ABS/runs/$rid.stderr"
  RUN_EXIT=$?
}

run_advanced() {
  local rid="$1"; shift
  local parallel_env="$1"; shift
  log "$rid: zyal-advanced-reasoning-live-local (parallel=$parallel_env)"
  (
    export JEKKO_ZYAL_LIVE=1 JEKKO_KEY_SOURCE_POLICY=users-only JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}"
    [[ "$parallel_env" = "on" ]] && export JEKKO_REASONING_PARALLEL=1
    /usr/bin/time -v -o "$BATCH_ABS/runs/$rid.time" \
      rtk just zyal-advanced-reasoning-live-local "$rid"
  ) > "$BATCH_ABS/runs/$rid.stdout" 2> "$BATCH_ABS/runs/$rid.stderr"
  RUN_EXIT=$?
}

run_superreasoning() {
  local rid="$1"; shift
  local parallel_env="$1"; shift
  local timeout_secs="${1:-}"
  log "$rid: zyal-superreasoning-live-local (parallel=$parallel_env, timeout=${timeout_secs:-default})"
  (
    export JEKKO_ZYAL_LIVE=1 JEKKO_KEY_SOURCE_POLICY=users-only JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}"
    [[ "$parallel_env" = "on" ]] && export JEKKO_REASONING_PARALLEL=1
    [[ -n "$timeout_secs" ]] && export JEKKO_MODEL_CALL_TIMEOUT_SECS="$timeout_secs"
    /usr/bin/time -v -o "$BATCH_ABS/runs/$rid.time" \
      rtk just zyal-superreasoning-live-local "$rid"
  ) > "$BATCH_ABS/runs/$rid.stdout" 2> "$BATCH_ABS/runs/$rid.stderr"
  RUN_EXIT=$?
}

run_miniredis() {
  local rid="$1"
  local max_calls="${BATCH_MINIREDIS_MAX_CALLS:-}"
  local max_parallel="${BATCH_MINIREDIS_MAX_PARALLEL:-4}"
  local max_ticks="${BATCH_MINIREDIS_MAX_TICKS:-}"
  if [[ -z "$max_calls" ]]; then
    if [[ "${JEKKO_ZYAL_SERIOUS:-}" = "1" ]]; then
      max_calls=64
    else
      max_calls=12
    fi
  fi
  if [[ -z "$max_ticks" ]]; then
    if [[ "${JEKKO_ZYAL_SERIOUS:-}" = "1" ]]; then
      max_ticks=12
    else
      max_ticks=1
    fi
  fi
  local config="$BATCH_ABS/config/miniredis-batch.port-run.json"
  jq \
    --argjson max_calls "$max_calls" \
    --argjson max_parallel "$max_parallel" \
    '
      .worker_cap = $max_parallel
      | .advanced_reasoning.worker_cap = $max_parallel
      | .live_call_budget.max_calls = $max_calls
      | .live_call_budget.max_parallel = $max_parallel
    ' docs/ZYAL/examples/35-rust-redis-replacement-superreasoning.port-run.json \
    > "$config" || fail "failed to write MiniRedis batch config"
  log "$rid: zyal-miniredis live port-run (parallel=on, max_ticks=$max_ticks, max_calls=$max_calls)"
  (
    export JEKKO_ZYAL_LIVE=1 JEKKO_KEY_SOURCE_POLICY=users-only JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}" JEKKO_REASONING_PARALLEL=1
    /usr/bin/time -v -o "$BATCH_ABS/runs/$rid.time" \
      rtk cargo run --manifest-path crates/jekko-runner/Cargo.toml --locked -- \
        --repo . --run-id "$rid" \
        port-run --config "$config" \
        --live --max-ticks "$max_ticks"
  ) > "$BATCH_ABS/runs/$rid.stdout" 2> "$BATCH_ABS/runs/$rid.stderr"
  RUN_EXIT=$?
}

run_super_redis() {
  local rid="$1"
  log "$rid: zyal-super-redis (plan-walk, no LLM)"
  JNOCCIO_UPSTREAM_KEY_SOURCE="${JNOCCIO_UPSTREAM_KEY_SOURCE:-users_pool}" \
    /usr/bin/time -v -o "$BATCH_ABS/runs/$rid.time" \
    rtk just zyal-super-redis "$rid" \
      > "$BATCH_ABS/runs/$rid.stdout" \
      2> "$BATCH_ABS/runs/$rid.stderr"
  RUN_EXIT=$?
}

# ---------- batch loop ---------------------------------------------------

# BATCH_ONLY filter: include all by default
batch_only_filter() {
  local rid="$1"
  local base
  base=$(base_run_id "$rid")
  if [[ -z "${BATCH_ONLY:-}" ]]; then return 0; fi
  for sel in $BATCH_ONLY; do
    if [[ "$rid" == "$sel"* || "$base" == "$sel"* ]]; then return 0; fi
  done
  return 1
}

start_fusion
start_observers

manifest_tmp=$(mktemp)
echo "[" > "$manifest_tmp"
first_entry=1

execute_run() {
  local idx="$1"
  local rid="${RUN_IDS[$idx]}"
  local label="${RUN_LABELS[$idx]}"
  local base
  base=$(base_run_id "$rid")

  if ! batch_only_filter "$rid"; then
    log "skipping $rid (BATCH_ONLY filter)"
    return 0
  fi

  if [[ "$idx" -eq 0 && "${BATCH_SKIP_SMOKE:-0}" = "1" ]]; then
    log "skipping r0 (BATCH_SKIP_SMOKE=1)"
    return 0
  fi

  log "==================== $rid : $label ===================="
  snapshot_balancer "before-$rid"
  [[ "$base" == "super-redis-r6-planwalk" ]] && snapshot_super_store "before-$rid"

  local started_at finished_at
  started_at=$(date -u +%s)

  case "$idx" in
    0) run_r0 "$rid" ;;
    1) run_advanced "$rid" "off" ;;
    2) run_advanced "$rid" "on" ;;
    3) run_superreasoning "$rid" "off" "" ;;
    4) run_superreasoning "$rid" "on" "" ;;
    5) run_miniredis "$rid" ;;
    6) run_super_redis "$rid" ;;
    7) run_superreasoning "$rid" "on" "300" ;;
  esac

  finished_at=$(date -u +%s)
  local duration_s=$((finished_at - started_at))
  log "$rid finished: exit=$RUN_EXIT duration=${duration_s}s"
  if [[ "$RUN_EXIT" -ne 0 ]]; then
    BATCH_FAILURES=$((BATCH_FAILURES + 1))
    BATCH_FAILED_RUNS+=("$rid=$RUN_EXIT")
  fi

  snapshot_balancer "after-$rid"
  [[ "$base" == "super-redis-r6-planwalk" ]] && snapshot_super_store "after-$rid"

  # Copy per-run events.jsonl if it exists
  local ev_src
  ev_src=$(events_path_for "$rid")
  if [[ -f "$ev_src" ]]; then
    cp "$ev_src" "$BATCH_ABS/runs/$rid.events.jsonl"
    log "captured $(wc -l < "$ev_src") events for $rid"
  else
    log "no events.jsonl for $rid at $ev_src"
    : > "$BATCH_ABS/runs/$rid.events.jsonl"
  fi

  # Append to manifest
  if [[ "$first_entry" -eq 0 ]]; then
    echo "," >> "$manifest_tmp"
  fi
  first_entry=0
  jq -n \
    --arg rid "$rid" \
    --arg label "$label" \
    --argjson exit_code "$RUN_EXIT" \
    --argjson started_at "$started_at" \
    --argjson finished_at "$finished_at" \
    --argjson duration_s "$duration_s" \
    --argjson events_count "$(wc -l < "$BATCH_ABS/runs/$rid.events.jsonl" 2>/dev/null || echo 0)" \
    '{run_id:$rid,label:$label,exit_code:$exit_code,started_at:$started_at,finished_at:$finished_at,duration_s:$duration_s,events_count:$events_count}' \
    >> "$manifest_tmp"
}

batch_started_at=$(date -u +%s)
for i in 0 1 2 3 4 5 6 7; do
  execute_run "$i"
done
batch_finished_at=$(date -u +%s)

echo "]" >> "$manifest_tmp"

jq -n \
  --arg batch_dir "$BATCH_DIR" \
  --arg utc_stamp "$UTC_STAMP" \
  --argjson started_at "$batch_started_at" \
  --argjson finished_at "$batch_finished_at" \
  --argjson runs "$(cat "$manifest_tmp")" \
  '{batch_dir:$batch_dir, utc_stamp:$utc_stamp, started_at:$started_at, finished_at:$finished_at, total_duration_s:($finished_at-$started_at), runs:$runs}' \
  > "$BATCH_ABS/manifest.json"

rm -f "$manifest_tmp"

log "==================== batch complete ===================="
log "manifest: $BATCH_DIR/manifest.json"
log "next: scripts/zyal-live-report.sh $BATCH_DIR"
if [[ "$BATCH_FAILURES" -ne 0 ]]; then
  fail "batch recorded $BATCH_FAILURES failed run(s): ${BATCH_FAILED_RUNS[*]}"
fi
