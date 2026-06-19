use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "jekko-runner",
    version,
    about = "Forever-runner that drains jankurai findings to zero across worktree workers."
)]
pub(crate) struct Cli {
    /// Repo root. Defaults to the current working directory.
    #[arg(long, default_value = ".", global = true)]
    pub(crate) repo: PathBuf,

    /// Unique run id. Used for branch / worktree / receipt namespacing. Random if omitted.
    #[arg(long, env = "JANKURAI_RUN_ID", global = true)]
    pub(crate) run_id: Option<String>,

    /// Worker pool size. Resolved to min(this, 20, jnoccio.spawn_batch_limit) at runtime.
    #[arg(long, default_value_t = 5)]
    pub(crate) pool_size: usize,

    /// Integration branch that worker branches rebase onto. Defaults to `zyal/<run_id>/integration`.
    #[arg(long)]
    pub(crate) integration_branch: Option<String>,

    /// Allow starting against a dirty working tree (will stash with audit trail).
    #[arg(long)]
    pub(crate) allow_dirty: bool,

    /// Do not invoke jankurai audit / git mutations. Useful in CI smoke tests.
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Run a single tick then exit (instead of looping forever).
    #[arg(long)]
    pub(crate) once: bool,

    /// Focused runner command. Omitted means the legacy jankurai tick loop.
    #[command(subcommand)]
    pub(crate) command: Option<RunnerCommand>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum RunnerCommand {
    /// Exercise the model client and persist a model outcome receipt.
    ModelSmoke(ModelSmokeArgs),
    /// Run one durable generic port workflow tick.
    PortRun(PortRunArgs),
    /// Run one ZYAL Hero/Judge prompt-evolution workflow.
    HeroJudgeRun(HeroJudgeRunArgs),
}

#[derive(Args, Debug)]
pub(crate) struct ModelSmokeArgs {
    /// Prompt to send to the model client.
    #[arg(long)]
    pub(crate) prompt: String,
    /// Use the live Jekko runtime instead of the fake deterministic client.
    #[arg(long)]
    pub(crate) live: bool,
    /// Provider override for live calls.
    #[arg(long)]
    pub(crate) provider: Option<String>,
    /// Model override for live calls.
    #[arg(long)]
    pub(crate) model: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct PortRunArgs {
    /// JSON or TOML port workflow config.
    #[arg(long)]
    pub(crate) config: PathBuf,
    /// Use the live Jekko runtime for planning.
    #[arg(long)]
    pub(crate) live: bool,
    /// Provider override for live calls.
    #[arg(long)]
    pub(crate) provider: Option<String>,
    /// Model override for live calls.
    #[arg(long)]
    pub(crate) model: Option<String>,
    /// Maximum ticks to run.
    #[arg(long)]
    pub(crate) max_ticks: Option<u64>,
    /// Seconds between ticks when running multiple ticks.
    #[arg(long, default_value_t = 30)]
    pub(crate) tick_interval_secs: u64,
    /// Stop when this file exists.
    #[arg(long)]
    pub(crate) stop_file: Option<PathBuf>,
    /// Run until stopped. Default for this binary remains one tick.
    #[arg(long)]
    pub(crate) forever: bool,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct HeroJudgeRunArgs {
    /// ZYAL runbook path.
    #[arg(long)]
    pub(crate) zyal: PathBuf,
    /// Use live Jekko runtime model calls.
    #[arg(long)]
    pub(crate) live: bool,
    /// Provider override for live calls.
    #[arg(long)]
    pub(crate) provider: Option<String>,
    /// Model override for live calls.
    #[arg(long)]
    pub(crate) model: Option<String>,
    /// Override maximum generations for smoke/proof runs.
    #[arg(long)]
    pub(crate) max_generations: Option<usize>,
    /// Number of sequential trials to run for plot-ready series data.
    #[arg(long, default_value_t = 1)]
    pub(crate) runs: usize,
}
