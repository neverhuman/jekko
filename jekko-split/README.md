# Jekko Split Family

`/home/ubuntu/jekko-split` is the local split-family root. Inside it,
`jekko/` is the public portal repository for downloads, examples, docs, and
contributor bootstrap. The other directories are independent supporting repos.

The split root is intentionally not a top-level build workspace. Each repo is
meant to build on its own, with its own CI, Jankurai config, tests, and remotes.

Source of truth:

- `jekko-split/repos.manifest.toml`
- `jekko-split/AGENTS.md`

Repos in the split:

- `jekko` (portal)
- `jekko-core`
- `jekko-mcp`
- `jekko-jnoccio`
- `jekko-jailgun`
- `jekko-zyal`
- `jekko-search`
- `jekko-memory`
- `jekko-agent`
- `jekko-deploy`

Repository conventions:

- `repos.manifest.toml` is machine-checkable: `path`, `name`, and `slug` must match per repo, `role` must be present, and remotes must use the canonical URLs below.
- `origin` and `jeryu` should point at `http://127.0.0.1:8787/git/jeryu/<repo>.git`
- `github` should point at `https://github.com/neverhuman/<repo>.git`
- import branches are fixed to `import/source-20260610` and `import/dirty-20260610`
- onboarding gates are explicit per repo: `remote-wired`, `ci-skeleton`, `jankurai-1.6.1`, and `audit-clean`
- `onboarded = true` is valid only when all four gates are true
- rollout waves are ordered `0`, `1`, `2`, then `3`; the manifest should stay grouped in that order
- child repos keep their own CI, owner/test maps, generated-zone rules, and jankurai standard
