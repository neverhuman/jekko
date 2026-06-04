#!/usr/bin/env bash
# Verify the core tools CI depends on are installed locally.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/ops/ci/lib.sh"

missing=()
warnings=()
ok=()

check_cmd() {
  local name="$1"
  local install_hint="$2"
  if command -v "$name" >/dev/null 2>&1; then
    local version
    version=$("$name" --version 2>&1 | head -1 || true)
    ok+=("$name — ${version:-installed}")
  else
    missing+=("$name — install: $install_hint")
  fi
}

check_cmd cargo "https://rustup.rs"
check_cmd rustc "https://rustup.rs"
check_cmd npm "https://nodejs.org or fnm"
check_cmd node "https://nodejs.org or fnm"
check_cmd just "brew install just"
check_cmd rtk "see /Users/bentaylor/.codex/RTK.md"
check_cmd gh "brew install gh"

if command -v gh >/dev/null 2>&1; then
  if ! gh auth token >/dev/null 2>&1; then
    missing+=("gh auth token — run `gh auth login` so the clean-room PR policy dry-run can authenticate")
  fi
fi

check_cmd jq "brew install jq"
check_cmd rg "brew install ripgrep"
check_cmd awk "system"
check_cmd python3 "brew install python"
check_cmd gitleaks "brew install gitleaks"
check_cmd syft "brew install syft"
check_cmd latexmk "brew install --cask mactex (or texlive on linux)"
check_cmd jankurai "cargo install --git https://github.com/neverhuman/jankurai --tag v1.5.1 --locked jankurai"

if command -v jankurai >/dev/null 2>&1; then
  version="$(jankurai --version 2>&1 | head -1 || true)"
  case "$version" in
    *"jankurai 1.5.1"*) ;;
    *)
      printf 'expected jankurai 1.5.1, got: %s\n' "${version:-unknown}" >&2
      exit 1
      ;;
  esac
fi

if command -v cargo >/dev/null 2>&1; then
  cargo audit --version >/dev/null 2>&1 || warnings+=("cargo-audit missing (run `cargo install cargo-audit --locked`)")
  cargo clippy --version >/dev/null 2>&1 || warnings+=("cargo-clippy missing (run `rustup component add clippy`)")
  cargo fmt --version >/dev/null 2>&1 || warnings+=("cargo-fmt missing (run `rustup component add rustfmt`)")
fi

printf 'CI-local prerequisites\n\n'
if [[ ${#ok[@]} -gt 0 ]]; then
  printf 'OK:\n'
  for entry in "${ok[@]}"; do printf '  ✓ %s\n' "$entry"; done
  printf '\n'
fi
if [[ ${#warnings[@]} -gt 0 ]]; then
  printf 'Warnings:\n'
  for entry in "${warnings[@]}"; do printf '  ! %s\n' "$entry"; done
  printf '\n'
fi
if [[ ${#missing[@]} -gt 0 ]]; then
  printf 'Missing:\n'
  for entry in "${missing[@]}"; do printf '  ✗ %s\n' "$entry"; done
  printf '\nInstall the missing tools above, then re-run `just ci-doctor`.\n'
  exit 1
fi

printf 'All CI prerequisites installed. The workflow wrappers in `ops/ci/`, `scripts/ci-local.sh`, and `just ci-local-pr-dry-run` should be runnable now.\n'
