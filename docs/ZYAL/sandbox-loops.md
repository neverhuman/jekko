# Sandbox Loops

A **sandbox loop** is a declarative ZYAL surface (Profile B, target=toml) that
lets agents execute experimental code in a disposable workspace, completely
outside the main git tree, with permission allowlists, captured logs, and a
patch-export step at the end.

Canonical spec: [`generated/sandbox-lanes.toml`](../../generated/sandbox-lanes.toml)
(generated from `agent/zyal/sandbox-lanes.zyal` via `zyalc`).

Runtime: [`crates/sandboxctl/`](../../crates/sandboxctl/).

## Why sandbox loops?

Without sandboxing, an agent that runs `npm install` or `cargo build` mutates
the live tree. Even with sandboxing-by-convention (worktrees, temp dirs)
nothing in the standard ensures:

- the command surface is bounded (allowlist),
- workspace and env are isolated (HOME/TMPDIR/XDG_CACHE_HOME),
- every command's stdout/stderr/exit/changed-files is captured for review,
- the result is delivered as a patch the human reviews before merge.

The sandbox-loop function makes all of that **declarative**: an agent who
picks a lane gets the guarantees by construction.

## Three backends

| Backend | Hosts | Isolation level |
|---|---|---|
| `worktree` | macOS + Linux | git worktree + fake env (no kernel namespaces) |
| `bubblewrap` | Linux only | process/IPC/UTS/net namespaces, ro-bind /usr, tmpfs /tmp |
| `docker` / `podman` | macOS + Linux (engine required) | container FS + cgroups + `--network none` |

Selection happens in the lane spec via `runtime.backend`. `sandboxctl` probes
the backend at `create` time and refuses with an actionable message if the
host can't honour it (e.g. bwrap on macOS).

## Lifecycle

```
# 1. Set up workspace + env + index entry (returns run_id)
sandboxctl create experiment-worktree

# 2. Run commands through the wrapper (permission-gated)
sandboxctl run <run-id> -- just check
sandboxctl run <run-id> -- bun --cwd packages/jekko test test/auth/

# 3. Inspect what happened
sandboxctl status <run-id>

# 4. Export a reviewable patch
sandboxctl export <run-id>     # writes ~/.local/share/agent-sandboxes/<id>/patch.diff

# 5. Teardown
sandboxctl destroy <run-id>
```

The `tools/sandbox-wrap.sh` helper bundles steps 1+2 for lane-driven workflows
(`lane.command = "tools/sandbox-wrap.sh --lane <name> -- <argv>"`).

## Permission gating

Every `sandboxctl run` call is matched against
`commands.allowed_patterns` (whitelist; must match at least one) and
`commands.denied_patterns` (blacklist; matches lose). The check uses globset
patterns operating on the joined argv. Denial wins on intersection:

- `allowed = ["cargo *"]` + `denied = ["cargo install*"]`
- `cargo build` → **allow**
- `cargo install foo` → **deny (exit 126)**

Each denied attempt is recorded as `.agent/runs/<cmd_id>.denied` so reviewers
can audit attempted but blocked commands.

## Adding a new lane

1. Edit `agent/zyal/sandbox-lanes.zyal`. Choose `runtime.backend`, fill `commands`
   carefully (start strict, expand later).
2. Run `cargo run -p zyalc -- compile agent/zyal/sandbox-lanes.zyal` to regenerate
   `generated/sandbox-lanes.toml`. CI fails if you commit a `.zyal` edit without
   the regenerated TOML.
3. Validate locally: `cargo run -p sandboxctl -- validate`.
4. Smoke the new lane: `tools/sandbox-wrap.sh --lane <name> -- echo ok`.

## CI integration

`.github/workflows/jankurai.yml` runs:

- `zyalc compile --all --check` — fails on drift between `.zyal` and target.
- `cargo test -p sandboxctl --tests` — runs all spec/permission/runid/cli/smoke layers.
- `sandbox-backends` matrix — exercises the real backend per host
  (bubblewrap + docker on Linux, worktree on macOS).

## Limits in v1

- No `cargo mutants` blocking gate.
- Only `agent/zyal/*.zyal` and `agent/workflows/*.zyal` are discovered
  by `zyalc compile --all`. Additional sources require listing.
- Default sandbox root is `~/.local/share/agent-sandboxes/{run_id}/`. Override
  via the lane's `sandbox_root` field or the `SANDBOXCTL_ROOT` env var.
- Patch export currently captures tracked-file diffs; pass
  `--include-untracked` to also bundle untracked-non-ignored files.
- Log rotation is manual. Run `sandboxctl list --active` periodically and
  `sandboxctl destroy --keep-logs` once a sandbox is reviewed.
