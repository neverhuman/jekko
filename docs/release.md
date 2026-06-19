# Release

Jekko releases are Rust-native. The release flow produces a tagged binary,
published package artifacts, release notes, and proof receipts. There is no
JS publish step in the shipping path.

## Version source

The version comes from the Git tag, for example `v2.0.0`. That tag is the
source of record for the published release, and the workspace
`Cargo.toml` `[workspace.package].version` must match it.

## Release staging

The release gate requires security proof, backup/restore readiness, monitoring
receipts, rollback guidance, and rate limit or abuse-control review before any
tag is promoted.

1. Create the signed annotated tag:

   ```sh
   git tag -s vX.Y.Z -m "vX.Y.Z"
   ```

2. Build the host release binary:

   ```sh
   cargo build -p jekko-cli --release --locked
   ```

   The binary lands at `target/release/jekko`.

3. Smoke the binary:

   ```sh
   target/release/jekko --version
   target/release/jekko --help
   ```

4. Run the release proof lanes before publishing:

   - `ops/ci/parity.sh`
   - `just security`
   - `just tui-ci`
   - `cargo test --workspace --locked --no-fail-fast`
   - `cargo run -p xtask -- baseline-diff --threshold 80`
   - `cargo run -p xtask -- guard-forbidden-runtime --mode advisory`

   Capture the output for each lane in the release notes.

5. Stage the publication flow through the existing scripts:

   - `ops/ci/publish-version.sh` cuts the release tag and finalizes the GitHub release metadata.
   - `ops/ci/publish.sh` publishes the package bundle to `dist/` and, for `latest`, emits the release artifacts.
   - `cargo run -p xtask -- publish-release-packages --dist-root ./dist --tag <channel>` is the package publication command the script invokes.
   - `cargo run -p xtask -- publish-release-artifacts --version vX.Y.Z --channel latest` attaches the versioned release artifacts when the channel is `latest`.

   The `xtask release package` and `xtask release attach` subcommands are
   still dry-run stubs; the shell scripts above are the executable release
   entrypoints today.

6. Assemble the release bundle:

   - `target/release/jekko`
   - `SHA256SUMS`
   - release notes
   - proof receipts under `target/jankurai/`:
     - `repo-score.json`
     - `repo-score.md`
     - `repair-queue.jsonl`
     - `score-history.jsonl`
     - `jankurai.sarif`
     - `summary.md`

7. Publish the release and then update any version-bound docs, including
   `docs/install.md`.

## Integrity and rollback

- Release artifacts are produced from the tagged commit SHA and the
  configured CI workflow.
- The checksum file is signed with the tag.
- If a release is bad, cut a patch release for the fix and mark the broken
  version accordingly in the notes.

## Future work

- Cross-build packaging stays out of this doc until a concrete lane exists.
- If `cross` or the Nix flake becomes the supported multi-target path,
  document the exact staging script here before relying on it for a ship.
