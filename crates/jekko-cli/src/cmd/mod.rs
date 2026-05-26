//! Subcommand implementations.
//!
//! Each module owns a `*Args` struct (for `clap::Subcommand`) and a `run`
//! function returning `anyhow::Result<()>`. Modules that still need runtime
//! glue keep their messages explicit about pending integration so the CLI
//! surface stays readable while the backing services land.
#![allow(missing_docs)]

pub mod acp;
pub mod agent;
pub mod daemon;
pub mod db;
pub mod debug;
pub mod export;
pub mod github;
pub mod import;
pub mod jankurai;
pub mod jnoccio;
pub mod keys;
pub mod mcp;
pub mod models;
pub mod plugin;
pub mod port_run;
pub mod pr;
pub mod providers;
pub mod run;
pub mod serve;
pub mod session;
pub mod stats;
pub mod tui;
pub mod uninstall;
pub mod upgrade;
pub mod watch;
