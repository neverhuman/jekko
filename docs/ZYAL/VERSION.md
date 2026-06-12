# ZYAL Version Contract

- Contract version: `2.6.0`
- Canonical schema: [`SPEC.md`](SPEC.md)
- Release tag: `v1.0.3`
- Runtime sentinel version: `<<<ZYAL v1:daemon ...>>>`
- `research.version`: `v1`

## Jankurai host block (2.6.0)

ZYAL now has a first-class `jankurai:` block. It is host-enforced by Jekko,
not preview-only: the daemon can run `jankurai audit`, build repair plans,
ingest findings into durable tasks, route risky packets to incubator lanes,
verify candidates, block regressions, roll back unverified work, checkpoint,
and compare the branch against `origin/main`.

The runtime sentinel remains `v1`; only the ZYAL contract and preview surface
advance to `2.6.0`.

## Compatibility rules

- `VERSION.md` is release metadata only. The human-facing schema lives in `SPEC.md`.
- Keep `ZYAL_CONTRACT_VERSION` at `2.6.0` until a parser/schema change requires a real contract bump.
- Adding or removing schema keys is a compatibility event and needs parser, preview, docs, and tests to move together.
- The runtime sentinel stays at `v1` until the shipped daemon envelope changes.

## Extension migration (2.5.0)

The canonical extension changes from `.zyal.yml` to bare `.zyal`. All
existing example/listing files have been renamed; the contained syntax,
sentinels, and YAML body are unchanged. The Profile A (runbook) format
continues to use the existing `<<<ZYAL v1:daemon ...>>>` sentinels — there
is no pragma for runbooks.

Two new profiles are introduced for declarative-but-non-runbook ZYAL files:

- **Profile B — declarative** (`# zyal: declarative target=toml schema=<name>@<ver>`)
  compiles to TOML. First use: `agent/sandbox-lanes.zyal` → `generated/sandbox-lanes.toml`.
- **Profile C — workflow** (`# zyal: declarative target=github-workflow schema=actions/workflow@<ver>`)
  compiles to GitHub Actions YAML under `.github/workflows/`.

The compiler binary is `zyalc` (`crates/zyalc/`).

## Compatibility rules

- Existing `.zyal` runbook files (formerly `.zyal.yml`) remain valid unless a
  new unknown top-level key or nested key is introduced.
- Profile B/C files must declare their pragma on the first non-blank line.
- Generated targets (`.toml` / `.yml` emitted by `zyalc`) include a
  `# zyalc: sha256=<hash>` trailer. CI's drift detector compares the trailer
  against a fresh compile to catch hand-edits of generated output.

The runtime sentinel version stays at `v1` for the shipped block shape. The
contract version tracks the ZYAL documentation and preview surface, not the
embedded YAML `version:` field.
