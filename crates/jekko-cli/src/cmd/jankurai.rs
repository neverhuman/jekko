//! `jekko jankurai` — passthrough to the external `jekko-runner` binary.
//!
//! Mirrors `packages/jekko/src/cli/cmd/jankurai/index.ts`. We don't link the
//! library directly: jankurai ships its own CLI which is the canonical entry
//! point. This wrapper just forwards remaining args.

use std::process::Command;

use anyhow::{Context, Result};
use clap::Args;

use crate::cli::GlobalOpts;

#[derive(Args, Debug, Default)]
pub struct JankuraiArgs {
    /// Args forwarded verbatim to `jekko-runner`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub forwarded: Vec<String>,
}

pub fn run(_global: &GlobalOpts, args: &JankuraiArgs) -> Result<()> {
    let mut cmd = Command::new("jekko-runner");
    cmd.args(&args.forwarded);
    let status = cmd
        .status()
        .context("failed to spawn jekko-runner; is it on PATH?")?;
    if !status.success() {
        let code = status.code().unwrap_or(1);
        std::process::exit(code);
    }
    Ok(())
}
