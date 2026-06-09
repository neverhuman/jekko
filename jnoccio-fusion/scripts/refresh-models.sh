#!/usr/bin/env bash
# refresh-models.sh — model-freshness check + prune for jnoccio-fusion.
#
# Runs `provider_probe` against the registry and removes models that are
# *definitively* dead (e.g. 404 ModelUnavailable / model-not-found) from
# models.json. Conservative: never prunes on transient errors (ContextOverflow,
# 429, timeouts). Time-gated (default: once / 24h) so the gateway can fire it on
# every cold start cheaply. A timestamped backup of models.json is always kept.
#
# Invoked by the gateway at startup as:  refresh-models.sh --config <server.json> --env-file <.env>
# Manual full run:                       refresh-models.sh --force
set -uo pipefail

BUNDLE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG="$BUNDLE/server.json"
ENVFILE="$BUNDLE/.env.jnoccio"
FORCE=0
INTERVAL_HOURS="${JNOCCIO_REFRESH_INTERVAL_HOURS:-24}"

while [ $# -gt 0 ]; do
  case "${1:-}" in
    --config)   CONFIG="${2:-}"; shift 2;;
    --env-file) ENVFILE="${2:-}"; shift 2;;
    --force)    FORCE=1; shift;;
    *)          shift;;
  esac
done

STATE_DIR="$BUNDLE/state"; mkdir -p "$STATE_DIR"
LOG="$STATE_DIR/refresh-models.log"
MARKER="$STATE_DIR/models_refreshed_at"
log(){ printf '[%s] %s\n' "$(date -u +%FT%TZ)" "$*" >> "$LOG"; }

# Single-flight: overlapping cold starts must not double-probe / race the write.
exec 9>"$STATE_DIR/.refresh.lock"
if ! flock -n 9; then log "another refresh in progress; skip"; exit 0; fi

# Time-gate (skip if recently refreshed, unless --force).
if [ "$FORCE" -ne 1 ] && [ -f "$MARKER" ]; then
  last=$(cat "$MARKER" 2>/dev/null || echo 0); now=$(date +%s)
  if [ $(( (now - last) / 3600 )) -lt "$INTERVAL_HOURS" ]; then
    log "registry fresh (< ${INTERVAL_HOURS}h); skip probe"; exit 0
  fi
fi

# Locate provider_probe (bundle first, then the crate's build output).
PROBE="$BUNDLE/provider_probe"
if [ ! -x "$PROBE" ]; then
  CRATE_ROOT="$(cd "$BUNDLE/.." && pwd)"   # jnoccio-fusion/
  for alt in "$CRATE_ROOT/target/release/provider_probe" \
             "$CRATE_ROOT/target/debug/provider_probe"; do
    [ -x "$alt" ] && { PROBE="$alt"; break; }
  done
fi
[ -x "$PROBE" ] || { log "provider_probe not found; skip"; exit 0; }

PROBE_JSON="$STATE_DIR/last-probe.json"
ARGS=(--config "$CONFIG"); [ -n "${ENVFILE:-}" ] && ARGS+=(--env-file "$ENVFILE")
log "probing via $PROBE ${ARGS[*]}"
if ! "$PROBE" "${ARGS[@]}" > "$PROBE_JSON" 2>>"$LOG"; then
  log "probe run failed; models.json left unchanged"; exit 1
fi

# Prune (conservative) via python; rewrites models.json atomically with backup.
python3 - "$CONFIG" "$PROBE_JSON" "$LOG" <<'PY'
import json, sys, os, re, time, shutil
config_path, probe_path, log_path = sys.argv[1:4]
def log(m):
    with open(log_path, "a") as f:
        f.write(f"[{time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}] {m}\n")

cfg = json.load(open(config_path))
mfile = cfg.get("models_file")
if not mfile:
    log("no models_file in config; abort"); print("PRUNED=0"); sys.exit(0)
if not os.path.isabs(mfile):
    mfile = os.path.join(os.path.dirname(os.path.abspath(config_path)), mfile)

records = json.load(open(probe_path)).get("records", [])
# Definitively dead (model no longer exists). Conservative allow-list of signals.
DEAD = re.compile(r'(model[_ ]?unavailable|model[_ ]?not[_ ]?found|no such model|does not exist|unknown model|\b404\b)', re.I)
# Never prune these — transient / request-shaped.
TRANSIENT = re.compile(r'(context[_ ]?overflow|context length|\b429\b|rate[_ ]?limit|timeout|timed out|connection|temporarily|\b5\d\d\b)', re.I)

dead = sorted({r.get("visible_id") for r in records
               if r.get("status") == "error"
               and not TRANSIENT.search(r.get("error") or "")
               and DEAD.search(r.get("error") or "")
               and r.get("visible_id")})

if not dead:
    log(f"probe ok: {len(records)} records, 0 dead; no prune"); print("PRUNED=0"); sys.exit(0)

reg = json.load(open(mfile)); before = len(reg.get("models", []))
dead_set = set(dead)  # dead holds probe visible_ids: "<provider>/<id>"
keep = [m for m in reg["models"] if f"{m.get('provider')}/{m.get('id')}" not in dead_set]
bak = f"{mfile}.bak.{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}"
shutil.copy2(mfile, bak)
reg["models"] = keep
tmp = mfile + ".tmp"; json.dump(reg, open(tmp, "w"), indent=2); os.replace(tmp, mfile)
log(f"pruned {before-len(keep)} dead model(s): {dead}; {before}->{len(keep)}; backup={os.path.basename(bak)}")
print(f"PRUNED={before-len(keep)}")
PY

date +%s > "$MARKER"
log "refresh complete"
