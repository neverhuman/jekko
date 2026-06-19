//! `jekko zyal-dispatch` — compile a `target=flowgraph` `.zyal` to its
//! byte-deterministic FlowGraph IR and dispatch the graph's executable nodes
//! through the runner kernels (M8 routing topology + M12 feed ticks) on the run's
//! real event spine. This is the seam that turns the compiled language into
//! actual gated, receipted, replayable execution.
//!
//! Non-executable / runtime-fed nodes (agents, tools, tournaments, gates) are
//! skipped here — they run via the agent/tool runtime paths once their inputs
//! exist; see `jekko_runner::dispatch::node_to_dispatch`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use jekko_runner::dispatch::dispatch_flowgraph;
use jekko_runner::run_store::SourceMode;

use crate::cli::GlobalOpts;

/// `jekko zyal-dispatch` arguments.
#[derive(Args, Debug)]
pub struct ZyalDispatchArgs {
    /// Path to a `target=flowgraph` `.zyal` manifest.
    #[arg(long, value_name = "PATH")]
    pub runbook: PathBuf,

    /// Override the run id (default: `<slug>-<epoch_ms>`).
    #[arg(long = "run-id", value_name = "ID")]
    pub run_id: Option<String>,

    /// Repo root the run is scoped to (event-ledger root). Defaults to the cwd.
    #[arg(long = "repo-root", value_name = "PATH")]
    pub repo_root: Option<PathBuf>,
}

pub fn run(_global: &GlobalOpts, args: &ZyalDispatchArgs) -> Result<()> {
    let repo_root = match &args.repo_root {
        Some(p) => p.clone(),
        None => std::env::current_dir().context("resolve cwd")?,
    };
    let repo_root = std::fs::canonicalize(&repo_root).unwrap_or(repo_root);

    // Compile the .zyal to its FlowGraph IR (errors if not a flowgraph profile).
    let ir_json = zyalc::compile::compile_flowgraph_ir(&args.runbook)
        .with_context(|| format!("compile flowgraph {}", args.runbook.display()))?;
    let ir: serde_json::Value =
        serde_json::from_str(&ir_json).context("parse compiled FlowGraph IR")?;

    let run_id = args.run_id.clone().unwrap_or_else(|| {
        let stem = args
            .runbook
            .file_stem()
            .and_then(|s| s.to_str())
            .map(slugify)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "flowgraph".to_string());
        format!("{stem}-{}", now_ms())
    });

    // Feeds use the deterministic fixture provider in every mode; tool adapters
    // (none dispatched from a static IR) honour JEKKO_TOOLS_FAKE for CI.
    let mode = if std::env::var("JEKKO_TOOLS_FAKE").as_deref() == Ok("1") {
        SourceMode::Fake
    } else {
        SourceMode::Live
    };

    let report = dispatch_flowgraph(&ir, &repo_root, &run_id, mode)
        .with_context(|| format!("dispatch flowgraph run {run_id}"))?;

    println!(
        "zyal-dispatch: run {run_id} — {} feed tick(s), {} route(s), {} tool(s), {} tournament(s), {} paper build(s)",
        report.feeds.len(),
        report.routes.len(),
        report.tools.len(),
        report.tournaments.len(),
        report.paper_builds.len(),
    );
    for (route_id, winner) in &report.routes {
        println!(
            "  route {route_id} → winner {}",
            winner.as_deref().unwrap_or("(none)")
        );
    }
    for receipt in &report.paper_builds {
        if !receipt.arxiv_tar.is_empty() {
            println!("  paper build → arxiv_tar {}", receipt.arxiv_tar);
        }
    }
    if !report.errors.is_empty() {
        eprintln!(
            "zyal-dispatch: {} frame emission error(s):",
            report.errors.len()
        );
        for e in &report.errors {
            eprintln!("  - {e}");
        }
    }
    println!("zyal-dispatch: events at target/zyal/runs/{run_id}/events.jsonl");
    Ok(())
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
