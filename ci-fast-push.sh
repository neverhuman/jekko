#!/usr/bin/env bash
set -euo pipefail

log() {
  printf 'ci-fast-push: %s\n' "$*"
}

if [ -d "${HOME:-}/.cargo/bin" ]; then
  PATH="${HOME}/.cargo/bin:${PATH}"
  export PATH
fi

rtk_passthrough_dir=""
cleanup_rtk_passthrough() {
  if [ -n "$rtk_passthrough_dir" ]; then
    rm -rf "$rtk_passthrough_dir"
  fi
}

if ! command -v rtk >/dev/null 2>&1; then
  rtk_passthrough_dir="$(mktemp -d)"
  cat >"${rtk_passthrough_dir}/rtk" <<'SH'
#!/usr/bin/env sh
exec "$@"
SH
  chmod +x "${rtk_passthrough_dir}/rtk"
  PATH="${rtk_passthrough_dir}:${PATH}"
  export PATH
  trap cleanup_rtk_passthrough EXIT
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

require_no_unstaged_changes() {
  if ! git diff --quiet --exit-code || [ -n "$(git ls-files --others --exclude-standard)" ]; then
    log "$1"
    git status --short
    exit 4
  fi
}

log "starting in $repo_root"
git fetch origin main
log "fetched origin/main $(git rev-parse --short origin/main)"
require_origin_main_ancestor

require_no_unstaged_changes "stage intended changes explicitly before running ci-fast-push"
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

require_no_unstaged_changes "jekko-fast left worktree drift; inspect and stage intentionally"

git fetch origin main
log "fetched origin/main $(git rev-parse --short origin/main) before push"
require_origin_main_ancestor

log "pushing HEAD $(git rev-parse --short HEAD) to main"
JANKURAI_SKIP_PREPUSH=1 git push origin HEAD:main
git fetch origin main

head_sha="$(git rev-parse HEAD)"
origin_sha="$(git rev-parse origin/main)"
if [ "$head_sha" != "$origin_sha" ]; then
  log "post-push verification failed: HEAD=$head_sha origin/main=$origin_sha"
  exit 3
fi

status_short="$(git status --short)"
if [ -n "$status_short" ]; then
  log "post-push tree is dirty"
  printf '%s\n' "$status_short"
  exit 4
fi

log "pushed HEAD to main $head_sha"
