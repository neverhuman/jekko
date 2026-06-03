#!/usr/bin/env bash
set -euo pipefail

log() {
  printf 'ci-fast-push: %s\n' "$*"
}

if [ -d "${HOME:-}/.cargo/bin" ]; then
  PATH="${HOME}/.cargo/bin:${PATH}"
  export PATH
fi

rtk_shim_dir=""
cleanup_rtk_shim() {
  if [ -n "$rtk_shim_dir" ]; then
    rm -rf "$rtk_shim_dir"
  fi
}

if ! command -v rtk >/dev/null 2>&1; then
  rtk_shim_dir="$(mktemp -d)"
  cat >"${rtk_shim_dir}/rtk" <<'SH'
#!/usr/bin/env sh
exec "$@"
SH
  chmod +x "${rtk_shim_dir}/rtk"
  PATH="${rtk_shim_dir}:${PATH}"
  export PATH
  trap cleanup_rtk_shim EXIT
  log "rtk not found; using passthrough wrapper"
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)"
cd "$repo_root"

utc_now() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

run_ci() {
  rtk just jekko-fast
}

require_origin_main_ancestor() {
  if ! git merge-base --is-ancestor origin/main HEAD; then
    log "origin/main is not an ancestor of HEAD; fetch/rebase main before pushing"
    log "HEAD=$(git rev-parse HEAD)"
    log "origin/main=$(git rev-parse origin/main)"
    exit 2
  fi
}

commit_staged_if_needed() {
  local message="$1"
  if git diff --cached --quiet --exit-code; then
    log "no staged changes to commit"
    return 0
  fi
  git commit -m "$message"
  log "committed $(git rev-parse --short HEAD) \"$message\""
}

log "starting in $repo_root"
git fetch origin main
log "fetched origin/main $(git rev-parse --short origin/main)"
require_origin_main_ancestor

git add --all -- .
commit_staged_if_needed "codex: fast push $(utc_now)"

log "jekko-fast starting"
set +e
run_ci
ci_status=$?
set -e
if [ "$ci_status" -ne 0 ]; then
  log "jekko-fast failed with exit $ci_status; preserving HEAD $(git rev-parse --short HEAD)"
  exit "$ci_status"
fi
log "jekko-fast passed"

git add --all -- .
if ! git diff --cached --quiet --exit-code; then
  commit_staged_if_needed "codex: fast push $(utc_now) ci drift"
fi

push_branch="${JAILGUN_CI_BRANCH:-codex/live-proof}"

git fetch origin main
log "fetched origin/main $(git rev-parse --short origin/main) before push"
require_origin_main_ancestor

log "pushing HEAD $(git rev-parse --short HEAD) to $push_branch"
JANKURAI_SKIP_PREPUSH=1 git push origin HEAD:"$push_branch"
git fetch origin "$push_branch"

head_sha="$(git rev-parse HEAD)"
origin_sha="$(git rev-parse "origin/$push_branch")"
if [ "$head_sha" != "$origin_sha" ]; then
  log "post-push verification failed: HEAD=$head_sha origin/$push_branch=$origin_sha"
  exit 3
fi

status_short="$(git status --short)"
if [ -n "$status_short" ]; then
  log "post-push tree is dirty"
  printf '%s\n' "$status_short"
  exit 4
fi

log "pushed HEAD to $push_branch $head_sha"
