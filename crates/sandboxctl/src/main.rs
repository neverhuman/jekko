use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod backend;
mod cli;
mod index;
mod permission;
mod runid;
mod spec;
mod spec_types;
mod wrapper;

/// Exit codes (sysexits.h + sandboxctl-specific).
pub mod exit {
    pub const OK: i32 = 0;
    pub const USAGE: i32 = 64;
    pub const LANE_NOT_FOUND: i32 = 65;
    pub const BACKEND_INIT_FAIL: i32 = 70;
    pub const FS_ERROR: i32 = 73;
    pub const CONFIG_ERROR: i32 = 78;
    pub const TIMEOUT: i32 = 124;
    pub const DENIED: i32 = 126;
    pub const NOT_FOUND: i32 = 127;
}

#[derive(Parser, Debug)]
#[command(
    name = "sandboxctl",
    version,
    about = "Declarative sandbox-loop runtime",
    long_about = "Read generated/sandbox-lanes.toml, set up disposable workspaces with permission allowlists, and execute commands through a wrapper that captures stdout/stderr/exit for agent feedback."
)]
struct Cli {
    /// Path to the sandbox-lanes file (default: generated/sandbox-lanes.toml).
    #[arg(long, global = true, env = "SANDBOXCTL_LANES")]
    lanes: Option<PathBuf>,

    /// Emit JSON output for machine consumption.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Create a fresh sandbox (workspace + env + index entry).
    Create(cli::create::Args),
    /// Execute a command inside an existing sandbox via the wrapper.
    Run(cli::run::Args),
    /// Show recent commands and current state.
    Status(cli::status::Args),
    /// Emit a patch + artifact bundle.
    Export(cli::export::Args),
    /// Teardown a sandbox (preserves logs if configured).
    Destroy(cli::destroy::Args),
    /// List active sandboxes.
    List(cli::list::Args),
    /// Schema-validate the lanes file.
    Validate(cli::validate::Args),
    /// Thin alias for `zyalc compile` (kept for ergonomics).
    CompileSpec(cli::compile_spec::Args),
}

fn main() {
    let cli = Cli::parse();
    let code = match dispatch(&cli) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("sandboxctl: {err:#}");
            exit::FS_ERROR
        }
    };
    std::process::exit(code);
}

fn dispatch(cli: &Cli) -> Result<i32> {
    match &cli.cmd {
        Cmd::Create(a) => cli::create::run(a, cli.lanes.as_deref(), cli.json),
        Cmd::Run(a) => cli::run::run(a, cli.lanes.as_deref(), cli.json),
        Cmd::Status(a) => cli::status::run(a, cli.json),
        Cmd::Export(a) => cli::export::run(a, cli.json),
        Cmd::Destroy(a) => cli::destroy::run(a, cli.json),
        Cmd::List(a) => cli::list::run(a, cli.json),
        Cmd::Validate(a) => cli::validate::run(a, cli.lanes.as_deref(), cli.json),
        Cmd::CompileSpec(a) => cli::compile_spec::run(a, cli.json),
    }
}
