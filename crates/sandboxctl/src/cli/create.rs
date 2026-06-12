use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::backend;
use crate::exit;
use crate::index;
use crate::runid;
use crate::spec;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Lane name from generated/sandbox-lanes.toml.
    pub lane: String,
    /// Optional run-id override; default is generated.
    #[arg(long)]
    pub run_id: Option<String>,
    /// Override the runtime backend chosen by the lane (worktree/bubblewrap/docker/podman).
    #[arg(long)]
    pub backend_override: Option<String>,
}

pub fn run(args: &Args, default_lanes: Option<&Path>, json: bool) -> Result<i32> {
    let path = match default_lanes {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from("generated/sandbox-lanes.toml"),
    };

    let doc = spec::load(&path).context("load lanes file")?;
    let lane = match doc.lanes.iter().find(|l| l.name == args.lane) {
        Some(l) => l.clone(),
        None => {
            eprintln!(
                "sandboxctl: lane '{}' not found in {}",
                args.lane,
                path.display()
            );
            return Ok(exit::LANE_NOT_FOUND);
        }
    };

    let run_id = match &args.run_id {
        Some(id) => id.clone(),
        None => runid::generate(),
    };
    let sandbox_root = expand_root(&doc.sandbox_root);

    let resolved = backend::Resolver::for_lane(&lane, args.backend_override.as_deref())?;
    if let Err(err) = resolved.probe() {
        eprintln!("sandboxctl: backend probe failed: {err}");
        return Ok(exit::BACKEND_INIT_FAIL);
    }

    let workspace = backend::Workspace {
        run_id: run_id.clone(),
        root: sandbox_root.join(&run_id),
        repo_subdir: PathBuf::from("workspace"),
    };

    resolved
        .create(&lane, &workspace)
        .with_context(|| format!("create workspace for lane {}", lane.name))?;

    let entry = index::Entry {
        run_id: run_id.clone(),
        lane_name: lane.name.clone(),
        backend: resolved.name().to_string(),
        root: workspace.root.clone(),
        created_at: runid::generate(),
        status: "ready".into(),
    };
    index::insert(&sandbox_root, &entry)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "run_id": run_id,
                "lane": lane.name,
                "backend": resolved.name(),
                "root": workspace.root.display().to_string(),
            })
        );
    } else {
        println!(
            "sandboxctl: created run_id={} lane={} backend={} root={}",
            run_id,
            lane.name,
            resolved.name(),
            workspace.root.display()
        );
    }
    Ok(exit::OK)
}

pub(crate) fn expand_root(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}
