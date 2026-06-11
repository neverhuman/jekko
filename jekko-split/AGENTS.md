# Jekko Split Family

- `/home/ubuntu/jekko-split` is the local split-family root; do not treat it as a Cargo workspace.
- `/home/ubuntu/jekko-split/jekko` is the public portal repo.
- Read `/home/ubuntu/jekko-split/jekko/agent/JANKURAI_STANDARD.md` before editing any file under this umbrella.
- The portal and supporting repos are independent Git repositories; keep the split map, onboarding state, and rollout ordering here.
- Keep `jekko-split/repos.manifest.toml` machine-checkable: path/name/slug must stay aligned, roles must be explicit, remotes must remain canonical, and import/onboarding gates must not be implicit.
- Preserve product code in the repos themselves. This metadata only tracks the repo family shape and local integration rules.
