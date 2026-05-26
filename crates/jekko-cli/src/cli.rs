//! Top-level `clap` definition for the `jekko` binary.
//!
//! Mirrors the TS entrypoint at `packages/jekko/src/index.ts`. Each
//! subcommand is implemented in [`crate::cmd`].

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::cmd;

/// Version string injected at compile time. Falls back to the workspace
/// version from `Cargo.toml` so `jekko --version` always prints something.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Optional git SHA injected by `build.rs` (or any wrapper that sets the env
/// var). When absent the `--version` output is just the package version.
pub const GIT_SHA: Option<&str> = option_env!("JEKKO_GIT_SHA");

/// Long version string combining the package version with a git SHA suffix
/// when available. Computed at compile time so it works on `const`-style help
/// output.
pub fn long_version() -> String {
    match GIT_SHA {
        Some(sha) if !sha.is_empty() => format!("{VERSION} ({sha})"),
        _ => VERSION.to_string(),
    }
}

/// Top-level CLI struct. Lives in the library so xtask can introspect the
/// command tree for help-parity checks.
#[derive(Parser, Debug)]
#[command(
    name = "jekko",
    version = VERSION,
    long_version = VERSION,
    about = "Jekko: a Rust-native agentic coding assistant",
    long_about = "Jekko: a Rust-native agentic coding assistant.\n\
                  \n\
                  Run `jekko` with no arguments to launch the TUI. Pass a\n\
                  prompt with `jekko run \"...\"` for one-shot execution,\n\
                  or `jekko serve` to start a headless HTTP server.\n\
                  \n\
                  Examples:\n\
                    jekko                       # launch the TUI\n\
                    jekko run \"refactor foo\"  # one-shot run\n\
                    jekko serve --port 8080     # headless server\n\
                    jekko session list          # list sessions\n\
                    jekko keys set OPENAI       # set an API key",
    propagate_version = true,
    disable_help_subcommand = true,
)]
pub struct Cli {
    /// Optional working directory; defaults to the current directory.
    #[arg(value_name = "PATH")]
    pub directory: Option<PathBuf>,

    /// Shared global flags. Flattened so they remain top-level on the help
    /// page (matches the TS yargs layout).
    #[command(flatten)]
    pub global: GlobalOpts,

    /// Resume the last session.
    #[arg(long = "continue", short = 'c')]
    pub r#continue: bool,

    /// Run one jankurai cycle headlessly (audit → fix → verify) and exit.
    /// Equivalent to `jekko jankurai --once`.
    #[arg(short = 'j', long = "jankurai")]
    pub jankurai: bool,

    /// Open a specific session by id.
    #[arg(short = 's', value_name = "SESSION_ID")]
    pub session: Option<String>,

    /// Subcommand to execute. Defaults to launching the TUI.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Global flags shared by every entrypoint.
#[derive(Args, Debug, Clone, Default)]
pub struct GlobalOpts {
    /// Print structured logs to stderr.
    #[arg(long = "print-logs", global = true)]
    pub print_logs: bool,

    /// Override the log level (TRACE, DEBUG, INFO, WARN, ERROR).
    #[arg(long = "log-level", value_name = "LEVEL", global = true)]
    pub log_level: Option<String>,

    /// Disable external plugins.
    #[arg(long = "pure", global = true)]
    pub pure: bool,

    /// Run without entering raw mode / alt-screen (PTY tests, headless smoke).
    #[arg(long = "headless", global = true)]
    pub headless: bool,

    /// Override the working directory before any I/O is performed.
    #[arg(long = "cwd", value_name = "PATH", global = true)]
    pub cwd: Option<PathBuf>,
}

/// The set of subcommands `jekko` understands. Module-level docs on each
/// branch live in the corresponding [`crate::cmd`] file.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Launch the interactive TUI explicitly. Same as running `jekko` with no
    /// arguments.
    Tui(cmd::tui::TuiArgs),

    /// One-shot non-interactive run.
    Run(cmd::run::RunArgs),

    /// Start the HTTP server.
    Serve(cmd::serve::ServeArgs),

    /// Manage sessions.
    Session(cmd::session::SessionArgs),

    /// Manage AI providers and credentials.
    #[command(alias = "auth")]
    Providers(cmd::providers::ProvidersArgs),

    /// List or inspect available models.
    Models(cmd::models::ModelsArgs),

    /// Manage canonical model API keys.
    Keys(cmd::keys::KeysArgs),

    /// Manage agents.
    Agent(cmd::agent::AgentArgs),

    /// Manage MCP (Model Context Protocol) servers.
    Mcp(cmd::mcp::McpArgs),

    /// ACP (Agent Client Protocol) integration.
    Acp(cmd::acp::AcpArgs),

    /// Jankurai integration commands.
    Jankurai(cmd::jankurai::JankuraiArgs),

    /// Manage local Jnoccio Fusion unlock and status.
    Jnoccio(cmd::jnoccio::JnoccioArgs),

    /// Manage the background daemon.
    Daemon(cmd::daemon::DaemonArgs),

    /// Manage plugins.
    #[command(alias = "plug")]
    Plugin(cmd::plugin::PluginArgs),

    /// Diagnostic and debug utilities.
    Debug(cmd::debug::DebugArgs),

    /// Import sessions from a backup.
    Import(cmd::import::ImportArgs),

    /// Export sessions to a file.
    Export(cmd::export::ExportArgs),

    /// Usage statistics.
    Stats(cmd::stats::StatsArgs),

    /// Pull-request helpers (passthrough to xtask).
    Pr(cmd::pr::PrArgs),

    /// GitHub event helpers (passthrough to xtask).
    Github(cmd::github::GithubArgs),

    /// Database tools.
    Db(cmd::db::DbArgs),

    /// Upgrade Jekko in place.
    Upgrade(cmd::upgrade::UpgradeArgs),

    /// Uninstall Jekko and remove data directories.
    Uninstall(cmd::uninstall::UninstallArgs),

    /// Tail a ZYAL run's NDJSON event stream and emit per-tick snapshot +
    /// remediation actions.
    Watch(cmd::watch::WatchArgs),

    /// Drive a 12-stage ZYAL SuperWorkflow run (compile -> seed -> walk
    /// waves). Phase H scaffold; the per-phase body is a stub until
    /// `jankurai-runner` is wired in.
    #[command(name = "port-run")]
    PortRun(cmd::port_run::PortRunArgs),
}
