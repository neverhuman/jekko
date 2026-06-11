//! `jekko` binary entry point.
//!
//! Mirrors `packages/jekko/src/index.ts`. The library crate (`jekko-cli`)
//! holds the actual command definitions and handler bodies; this file is
//! intentionally thin so the `cargo bin` target is easy to read.

use anyhow::Result;
use jekko_cli::run;

fn main() -> Result<()> {
    run(std::env::args_os()).map(|_| ())
}
