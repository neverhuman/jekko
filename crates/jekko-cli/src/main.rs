//! `jekko` binary entry point.
//!
//! Mirrors `packages/jekko/src/index.ts`. The library crate (`jekko-cli`)
//! holds the actual command definitions and handler bodies; this file is
//! intentionally thin so the `cargo bin` target is easy to read.

use anyhow::Result;
use clap::Parser;
use jekko_cli::cli::{Cli, Command};
use jekko_cli::{cmd, runtime};

fn main() -> Result<()> {
    let cli = Cli::parse();
    runtime::bootstrap(&cli.global)?;

    // Short-circuit: `-j` / `--jankurai` runs one cycle via jankurai-runner.
    if cli.jankurai {
        return cmd::jankurai::run(
            &cli.global,
            &cmd::jankurai::JankuraiArgs {
                forwarded: vec!["--once".to_string()],
            },
        );
    }

    match &cli.command {
        Some(Command::Tui(args)) => cmd::tui::run(&cli.global, args),
        Some(Command::Run(args)) => cmd::run::run(&cli.global, args),
        Some(Command::Serve(args)) => cmd::serve::run(&cli.global, args),
        Some(Command::Session(args)) => cmd::session::run(&cli.global, args),
        Some(Command::Providers(args)) => cmd::providers::run(&cli.global, args),
        Some(Command::Models(args)) => cmd::models::run(&cli.global, args),
        Some(Command::Keys(args)) => cmd::keys::run(&cli.global, args),
        Some(Command::Agent(args)) => cmd::agent::run(&cli.global, args),
        Some(Command::Mcp(args)) => cmd::mcp::run(&cli.global, args),
        Some(Command::Acp(args)) => cmd::acp::run(&cli.global, args),
        Some(Command::Jankurai(args)) => cmd::jankurai::run(&cli.global, args),
        Some(Command::Jnoccio(args)) => cmd::jnoccio::run(&cli.global, args),
        Some(Command::Daemon(args)) => cmd::daemon::run(&cli.global, args),
        Some(Command::Plugin(args)) => cmd::plugin::run(&cli.global, args),
        Some(Command::Debug(args)) => cmd::debug::run(&cli.global, args),
        Some(Command::Import(args)) => cmd::import::run(&cli.global, args),
        Some(Command::Export(args)) => cmd::export::run(&cli.global, args),
        Some(Command::Stats(args)) => cmd::stats::run(&cli.global, args),
        Some(Command::Pr(args)) => cmd::pr::run(&cli.global, args),
        Some(Command::Github(args)) => cmd::github::run(&cli.global, args),
        Some(Command::Db(args)) => cmd::db::run(&cli.global, args),
        Some(Command::Upgrade(args)) => cmd::upgrade::run(&cli.global, args),
        Some(Command::Uninstall(args)) => cmd::uninstall::run(&cli.global, args),
        Some(Command::Watch(args)) => cmd::watch::run(&cli.global, args),
        Some(Command::PortRun(args)) => cmd::port_run::run(&cli.global, args),
        None => {
            // Default: launch the TUI.
            let tui_args = cmd::tui::TuiArgs {
                r#continue: cli.r#continue,
                session: cli.session.clone(),
            };
            cmd::tui::run(&cli.global, &tui_args)
        }
    }
}
