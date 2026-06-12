use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::exit;
use crate::spec;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Path to the lanes file (defaults to --lanes / generated/sandbox-lanes.toml).
    pub path: Option<PathBuf>,
    /// Reject any warning that would normally be advisory.
    #[arg(long)]
    pub strict: bool,
}

pub fn run(args: &Args, default_lanes: Option<&Path>, json: bool) -> Result<i32> {
    let path = pick_path(args.path.as_deref(), default_lanes)?;
    let doc = match spec::load(&path) {
        Ok(d) => d,
        Err(err) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "path": path.display().to_string(),
                        "error": err.to_string(),
                    })
                );
            } else {
                eprintln!("sandboxctl: {err}");
            }
            return Ok(exit::CONFIG_ERROR);
        }
    };

    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "path": path.display().to_string(),
                "lanes": doc.lanes.len(),
                "schema_version": doc.schema_version,
            })
        );
    } else {
        println!(
            "sandboxctl: {} validated ({} lane(s), schema {})",
            path.display(),
            doc.lanes.len(),
            doc.schema_version
        );
    }
    let _ = args.strict;
    Ok(exit::OK)
}

fn pick_path(arg: Option<&Path>, default_lanes: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = arg {
        return Ok(p.to_path_buf());
    }
    if let Some(p) = default_lanes {
        return Ok(p.to_path_buf());
    }
    let default_path = PathBuf::from("generated/sandbox-lanes.toml");
    if default_path.exists() {
        Ok(default_path)
    } else {
        Err(anyhow::anyhow!(
            "no lanes file specified and generated/sandbox-lanes.toml not found"
        ))
        .context("validate")
    }
}
