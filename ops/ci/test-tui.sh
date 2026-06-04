#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

git config --global user.email "bot@jekko.ai"
git config --global user.name "jekko"
cargo build -p jekko-cli --locked
cargo run -p jekko-cli -- --version
cargo run -p jekko-cli -- --help
cargo test -p jekko-tui --locked --no-fail-fast

# Best-effort install of the `jankurai` binary so the real-audit render e2e
# below actually exercises (it skips cleanly when jankurai is absent, so a
# download failure must not fail the lane).
if ! command -v jankurai >/dev/null 2>&1; then
  (
    set -e
    ver="1.5.1"
    target="x86_64-unknown-linux-gnu"
    sha="a12dbb4a3805dee807fc101d4b073ac9386936b33c5579f606a655fe90d0bbac"
    tmp="$(mktemp -d)"
    arc="$tmp/jankurai-${ver}-${target}.tar.gz"
    curl -fsSL "https://github.com/neverhuman/jankurai/releases/download/v${ver}/jankurai-${ver}-${target}.tar.gz" -o "$arc"
    echo "${sha}  ${arc}" | sha256sum -c -
    tar -xzf "$arc" -C "$tmp"
    mkdir -p "${HOME}/.local/bin"
    install -m 0755 "$tmp/jankurai-${ver}-${target}/jankurai" "${HOME}/.local/bin/jankurai"
  ) || echo "jankurai install failed; the real-audit render e2e will skip"
fi
export PATH="${HOME}/.local/bin:${PATH}"

# Load-bearing guard for the PTY live-render fix: a `\r`/clear-line progress bar
# (e.g. /audit's jankurai scan) must collapse onto one updating line via the
# persistent vt100 emulator + ScreenUpdate replace path, not flood the
# transcript. Named explicitly so this coverage survives any rescoping of the
# broad test step above. The last entry is the real-audit end-to-end render test
# (`jankurai_audit_renders_final_score_through_pty`): it drives an actual
# `jankurai audit` through pty_runner and asserts the final screen shows
# `score=` and didn't flood. It skips cleanly when jankurai isn't on PATH.
cargo test -p jekko-tui --locked -- \
  terminal_collapses_carriage_return_progress_bar \
  terminal_keeps_newline_committed_lines \
  carriage_return_progress_collapses_to_single_line \
  screen_update_replaces_rather_than_appends \
  jankurai_audit_renders_final_score_through_pty
JEKKO_BIN="$(cargo run -p xtask -- host-binary-path)" cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml default_tui_paints_first_frame -- --nocapture
JEKKO_BIN="$(cargo run -p xtask -- host-binary-path)" cargo test --manifest-path crates/tuiwright-jekko-unlock/Cargo.toml --no-run
