#!/usr/bin/env bash
set -euo pipefail

ROOT="${JEKKO_SPLIT_ROOT:-${HOME}/jekko-split}"
REMOTE="github"

usage() {
  cat <<'EOF'
Usage: scripts/split-sync.sh [--remote github|jeryu] [--root PATH]

Clone or fast-forward the nine supporting Jekko split-family repositories.
The portal repository itself is not cloned by this script.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --remote)
      REMOTE="${2:-}"
      shift 2
      ;;
    --root)
      ROOT="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$REMOTE" in
  github|jeryu) ;;
  *)
    echo "--remote must be github or jeryu, got: $REMOTE" >&2
    exit 2
    ;;
esac

repos=(
  jekko-core
  jekko-mcp
  jekko-deploy
  jekko-jnoccio
  jekko-jailgun
  jekko-zyal
  jekko-search
  jekko-memory
  jekko-agent
)

jeryu_url() {
  printf 'http://127.0.0.1:8787/git/jeryu/%s.git\n' "$1"
}

github_url() {
  printf 'https://github.com/neverhuman/%s.git\n' "$1"
}

remote_url() {
  local repo="$1"
  case "$REMOTE" in
    github) github_url "$repo" ;;
    jeryu) jeryu_url "$repo" ;;
  esac
}

set_remote_url() {
  local dest="$1"
  local name="$2"
  local url="$3"
  if git -C "$dest" remote get-url "$name" >/dev/null 2>&1; then
    git -C "$dest" remote set-url "$name" "$url"
  else
    git -C "$dest" remote add "$name" "$url"
  fi
}

ensure_remotes() {
  local dest="$1"
  local repo="$2"
  local local_url
  local public_url
  local_url="$(jeryu_url "$repo")"
  public_url="$(github_url "$repo")"
  set_remote_url "$dest" origin "$local_url"
  set_remote_url "$dest" jeryu "$local_url"
  set_remote_url "$dest" github "$public_url"
}

sync_existing() {
  local dest="$1"
  local repo="$2"
  local selected="$3"

  if [[ -n "$(git -C "$dest" status --short)" ]]; then
    echo "$dest has local changes; refusing to update" >&2
    exit 1
  fi

  ensure_remotes "$dest" "$repo"
  git -C "$dest" fetch "$selected" main
  if git -C "$dest" rev-parse --verify --quiet refs/heads/main >/dev/null; then
    git -C "$dest" checkout main >/dev/null
    git -C "$dest" merge --ff-only FETCH_HEAD
  else
    git -C "$dest" checkout -B main FETCH_HEAD >/dev/null
  fi
}

clone_repo() {
  local dest="$1"
  local repo="$2"
  local url
  url="$(remote_url "$repo")"
  if [[ "$REMOTE" = "github" ]]; then
    git clone --origin github --branch main "$url" "$dest"
  else
    git clone --origin origin --branch main "$url" "$dest"
  fi
  ensure_remotes "$dest" "$repo"
}

mkdir -p "$ROOT"

for repo in "${repos[@]}"; do
  dest="${ROOT}/${repo}"
  if [[ -d "${dest}/.git" ]]; then
    echo "updating ${repo} from ${REMOTE}"
    sync_existing "$dest" "$repo" "$REMOTE"
  elif [[ -e "$dest" ]]; then
    echo "$dest exists but is not a Git checkout" >&2
    exit 1
  else
    echo "cloning ${repo} from ${REMOTE}"
    clone_repo "$dest" "$repo"
  fi
done
