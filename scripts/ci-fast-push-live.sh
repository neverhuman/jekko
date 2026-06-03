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
  log "rtk not found; using passthrough shim"
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)"
cd "$repo_root"

base_remote="${JAILGUN_CI_BASE_REMOTE:-${JAILGUN_CI_LOCAL_REMOTE:-jeryu}}"
push_remote="${JAILGUN_CI_PUSH_REMOTE:-${base_remote}}"
push_branch="${JAILGUN_CI_BRANCH:-main}"

lock_path="${repo_root}/.git/ci-fast-push-live.lock"
exec {live_push_lock_fd}>"$lock_path"
log "acquiring live push lock $lock_path"
flock "$live_push_lock_fd"
log "acquired live push lock $lock_path"

utc_now() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

run_ci() {
  rtk just jekko-fast
}

require_remote() {
  local remote="$1"
  if git remote get-url "$remote" >/dev/null 2>&1; then
    return 0
  fi

  log "missing git remote '$remote'"
  if [ "$remote" = "jeryu" ]; then
    log "add it with: git remote add jeryu ssh://git@localhost:2224/root/jekko.git"
  fi
  exit 2
}

require_remote_main_ancestor() {
  local remote="$1"
  if ! git merge-base --is-ancestor "${remote}/main" HEAD; then
    log "${remote}/main is not an ancestor of HEAD; fetch/rebase main before pushing"
    log "HEAD=$(git rev-parse HEAD)"
    log "${remote}/main=$(git rev-parse "${remote}/main")"
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
require_remote "$base_remote"
require_remote "$push_remote"

git fetch "$base_remote" main
log "fetched ${base_remote}/main $(git rev-parse --short "${base_remote}/main")"
require_remote_main_ancestor "$base_remote"

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

git fetch "$push_remote" "$push_branch" || true

git fetch "$base_remote" main
log "fetched ${base_remote}/main $(git rev-parse --short "${base_remote}/main") before push"
require_remote_main_ancestor "$base_remote"

if [ "$push_remote" = "origin" ] && [ "$push_branch" = "main" ] && [ "${JAILGUN_CI_PROMOTE_GITHUB_MAIN:-0}" != "1" ]; then
  log "refusing to push to origin/main without JAILGUN_CI_PROMOTE_GITHUB_MAIN=1"
  exit 2
fi

log "pushing HEAD $(git rev-parse --short HEAD) to ${push_remote}/${push_branch} with force-with-lease"
JANKURAI_SKIP_PREPUSH=1 git push --force-with-lease "$push_remote" HEAD:"$push_branch"
git fetch "$push_remote" "$push_branch"

head_sha="$(git rev-parse HEAD)"
remote_sha="$(git rev-parse "${push_remote}/${push_branch}")"
if [ "$head_sha" != "$remote_sha" ]; then
  log "post-push verification failed: HEAD=$head_sha ${push_remote}/${push_branch}=$remote_sha"
  exit 3
fi

status_short="$(git status --short)"
if [ -n "$status_short" ]; then
  log "post-push tree is dirty"
  printf '%s\n' "$status_short"
  exit 4
fi

log "pushed HEAD to ${push_remote}/${push_branch} $head_sha"
