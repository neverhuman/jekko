use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::backend;
use crate::exit;
use crate::index;
use crate::permission::{self, Decision};
use crate::spec;
use crate::wrapper;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Run-id returned by `create`.
    pub run_id: String,
    /// Override the lane-declared timeout (seconds).
    #[arg(long)]
    pub timeout: Option<u64>,
    /// Tail-N lines to emit on stdout/stderr after run.
    #[arg(long)]
    pub tail: Option<u32>,
    /// argv to execute inside the sandbox. Use `--` to separate from sandboxctl flags.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub argv: Vec<String>,
}

pub fn run(args: &Args, default_lanes: Option<&Path>, json: bool) -> Result<i32> {
    let path = match default_lanes {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from("generated/sandbox-lanes.toml"),
    };
    let doc = spec::load(&path).context("load lanes file")?;
    let sandbox_root = crate::cli::create::expand_root(&doc.sandbox_root);

    let entry = match index::find(&sandbox_root, &args.run_id)? {
        Some(e) => e,
        None => {
            eprintln!("sandboxctl: run_id '{}' not found", args.run_id);
            return Ok(exit::LANE_NOT_FOUND);
        }
    };

    let lane = doc
        .lanes
        .iter()
        .find(|l| l.name == entry.lane_name)
        .with_context(|| format!("lane '{}' missing from spec", entry.lane_name))?
        .clone();

    let matcher = permission::Matcher::new(
        &lane.commands.allowed_patterns,
        &lane.commands.denied_patterns,
    )?;

    let workspace = backend::Workspace {
        run_id: entry.run_id.clone(),
        root: entry.root.clone(),
        repo_subdir: PathBuf::from("workspace"),
    };

    let exec_args = wrapper::ExecArgs {
        argv: args.argv.clone(),
        timeout_seconds: args.timeout.unwrap_or(lane.runtime.timeout_seconds),
        tail_lines: args.tail.unwrap_or(lane.feedback.tail_lines),
    };

    match matcher.evaluate(&args.argv) {
        Decision::DeniedByPattern { pattern } => {
            wrapper::record_denial(&workspace, &args.argv, &pattern)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "decision": "denied",
                        "pattern": pattern,
                        "argv": args.argv,
                    })
                );
            } else {
                eprintln!("sandboxctl: command denied by pattern '{pattern}'");
            }
            return Ok(exit::DENIED);
        }
        Decision::NoAllowMatched => {
            wrapper::record_denial(&workspace, &args.argv, "<no-allow-matched>")?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "decision": "no_allow_matched",
                        "argv": args.argv,
                    })
                );
            } else {
                eprintln!(
                    "sandboxctl: command did not match any allowed pattern; denied by default"
                );
            }
            return Ok(exit::DENIED);
        }
        Decision::Allow => {}
    }

    let resolved = backend::Resolver::for_lane(&lane, None)?;
    let outcome = wrapper::dispatch(&resolved, &lane, &workspace, &exec_args)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
    } else {
        wrapper::print_outcome(&outcome);
    }
    Ok(outcome.exit_code)
}
