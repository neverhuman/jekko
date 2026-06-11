//! Jekko CLI library surface.
//!
//! This crate ships a `jekko` binary (see `src/main.rs`) and a small library
//! that exposes the parsed `Cli` shape so xtask/help-parity tooling can render
//! the command tree without re-parsing argv.
//!
//! The TypeScript predecessor lives at `packages/jekko/src/index.ts` and
//! `packages/jekko/src/cli/cmd/**`. Subcommand modules under [`cmd`] mirror
//! that layout one-to-one.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::ffi::OsString;

use anyhow::Result;
use clap::Parser;

pub mod cli;
pub mod cmd;
pub mod config_loader;
pub mod migration_ui;
pub mod runtime;

pub use cli::{Cli, Command, GlobalOpts};

/// Parse `argv`, bootstrap the runtime, and dispatch the selected command.
///
/// This keeps the binary entrypoint thin while giving the unification layer a
/// stable library API it can call from tests, wrappers, and future assembly
/// crates.
pub fn run<I, T>(argv: I) -> Result<i32>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::parse_from(argv);
    dispatch(cli)?;
    Ok(0)
}

fn dispatch(cli: Cli) -> Result<()> {
    runtime::bootstrap(&cli.global, cli.directory.as_deref())?;

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
