#!/usr/bin/env bash
# CI gate for the jekko-runner crate (PR3 of the enchanted-clock plan).
# Build + tests run against the local manifest with --locked so reproducible
# builds catch any unintended Cargo.lock drift.
set -euo pipefail

CRATE_MANIFEST="crates/jekko-runner/Cargo.toml"

echo "+ cargo build --manifest-path ${CRATE_MANIFEST} --locked"
cargo build --manifest-path "${CRATE_MANIFEST}" --locked

echo "+ cargo test --manifest-path ${CRATE_MANIFEST} --locked --tests --no-fail-fast"
cargo test --manifest-path "${CRATE_MANIFEST}" --locked --tests --no-fail-fast
