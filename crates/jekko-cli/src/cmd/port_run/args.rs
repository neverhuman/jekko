use anyhow::{bail, Result};

use super::PortRunArgs;

/// Validate `--live` preconditions. Refuses CI environments and requires the
/// `JEKKO_ZYAL_LIVE=1` opt-in so accidental invocations from automation can
/// not spend tokens. Called only when `args.live` is set.
pub(super) fn gate_live_mode() -> Result<()> {
    if env_is_truthy("CI") {
        bail!("--live refuses to run when CI=true; unset CI or run interactively to use live mode");
    }
    if !env_is_truthy("JEKKO_ZYAL_LIVE") {
        bail!("--live requires JEKKO_ZYAL_LIVE=1 (opt-in guard against accidental live runs)");
    }
    Ok(())
}

fn env_is_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

pub(super) fn validate_arg_combination(args: &PortRunArgs) -> Result<()> {
    let mode_count = [
        args.super_manifest.is_some(),
        args.resume.is_some(),
        args.status.is_some(),
        args.summarize.is_some(),
    ]
    .iter()
    .filter(|x| **x)
    .count();
    if mode_count == 0 {
        bail!(
            "provide one of --super <MANIFEST>, --resume <RUN_ID>, --status <RUN_ID>, or --summarize <RUN_ID>"
        );
    }
    if mode_count > 1 {
        bail!("--super, --resume, --status, and --summarize are mutually exclusive");
    }
    if args.dry_run && (args.resume.is_some() || args.status.is_some() || args.summarize.is_some())
    {
        bail!("--dry-run is only valid with --super");
    }
    Ok(())
}
