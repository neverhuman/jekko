//! `jekko mcp` — Model Context Protocol server management.
//!
//! v1 surface (ARY-2221, ADR-020): real `list`/`attach`/`detach`/`status`
//! over the stdio transport. SSE is recognized at the argument level but
//! refused at spawn time; `status` over sse will surface a structured error
//! pointing at the v2 issue. Config persists to `$JEKKO_HOME/mcp.toml`
//! (or `$HOME/.jekko/mcp.toml`); override with `--config <path>` on each
//! subcommand.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand};
use jekko_runtime::mcp::{
    default_mcp_config_path, load_or_empty, remove_server_entry, validate_server_name,
    write_server_entry, McpError, McpServerConfig, StdioClient,
};

use crate::cli::GlobalOpts;

pub mod presets;

#[derive(Args, Debug)]
pub struct McpArgs {
    #[command(subcommand)]
    pub command: McpCommand,
}

#[derive(Subcommand, Debug)]
pub enum McpCommand {
    /// List configured MCP servers.
    List(McpListArgs),
    /// Attach a new MCP server to the active config.
    Attach(McpAttachArgs),
    /// Detach an MCP server.
    Detach(McpDetachArgs),
    /// Probe a configured server: spawn, handshake, list its tools.
    Status(McpStatusArgs),
    /// Manage built-in MCP server presets.
    Preset(McpPresetArgs),
}

#[derive(Args, Debug)]
pub struct McpPresetArgs {
    #[command(subcommand)]
    pub command: McpPresetCommand,
}

#[derive(Subcommand, Debug)]
pub enum McpPresetCommand {
    /// List built-in presets.
    List,
    /// Add a built-in preset to the active config.
    Add(McpPresetAddArgs),
}

#[derive(Args, Debug)]
pub struct McpPresetAddArgs {
    #[command(flatten)]
    pub config: McpConfigOpt,

    /// Preset name (see `jekko mcp preset list`).
    pub name: String,

    /// Override the server name written into `mcp.toml`. Defaults to the
    /// preset's own name.
    #[arg(long = "as", value_name = "NAME")]
    pub rename: Option<String>,

    /// Overwrite an existing entry with the same server name.
    #[arg(long)]
    pub force: bool,
}

/// Shared flag for selecting the config file.
#[derive(Args, Debug, Clone, Default)]
pub struct McpConfigOpt {
    /// Override the config file path. Defaults to `$JEKKO_HOME/mcp.toml`
    /// or `$HOME/.jekko/mcp.toml`.
    #[arg(long = "config", value_name = "PATH")]
    pub config_path: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct McpListArgs {
    #[command(flatten)]
    pub config: McpConfigOpt,
}

/// `jekko mcp attach <name> <target> [-- <args...>]`
///
/// For stdio: `target` is the command, anything after `--` is its args.
///   Example: `jekko mcp attach aara python -- -m apps.mcp_server`
///
/// For sse: `target` is the URL. Reserved — refused at spawn time in v1.
#[derive(Args, Debug)]
pub struct McpAttachArgs {
    #[command(flatten)]
    pub config: McpConfigOpt,

    /// Server name. Letters, digits, `-`, `_` only, 1–64 chars.
    pub name: String,

    /// Transport. `stdio` (default) or `sse` (reserved for v2).
    #[arg(long, default_value = "stdio")]
    pub transport: String,

    /// For stdio: command to spawn. For sse: URL.
    pub target: String,

    /// For stdio only: arguments to pass to the spawned command. Everything
    /// after `--` is captured here, e.g.
    /// `jekko mcp attach aara python -- -m apps.mcp_server --transport stdio`.
    #[arg(last = true)]
    pub args: Vec<String>,

    /// Overwrite an existing entry with the same name.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct McpDetachArgs {
    #[command(flatten)]
    pub config: McpConfigOpt,

    /// MCP server name.
    pub name: String,
}

#[derive(Args, Debug)]
pub struct McpStatusArgs {
    #[command(flatten)]
    pub config: McpConfigOpt,

    /// MCP server name.
    pub name: String,

    /// Timeout (seconds) for the initialize + tools/list exchange. Falls
    /// back to the server's `default` timeout tier when unspecified.
    #[arg(long, value_name = "SECONDS")]
    pub timeout: Option<u64>,
}

pub fn run(_global: &GlobalOpts, args: &McpArgs) -> Result<()> {
    match &args.command {
        McpCommand::List(a) => {
            let p = resolve_config_path(a.config.config_path.as_deref())?;
            list(&p)
        }
        McpCommand::Attach(a) => {
            let p = resolve_config_path(a.config.config_path.as_deref())?;
            attach(&p, a)
        }
        McpCommand::Detach(a) => {
            let p = resolve_config_path(a.config.config_path.as_deref())?;
            detach(&p, &a.name)
        }
        McpCommand::Status(a) => {
            let p = resolve_config_path(a.config.config_path.as_deref())?;
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("build tokio runtime for mcp status")?;
            rt.block_on(status(&p, &a.name, a.timeout))
        }
        McpCommand::Preset(p) => match &p.command {
            McpPresetCommand::List => preset_list(),
            McpPresetCommand::Add(a) => {
                let path = resolve_config_path(a.config.config_path.as_deref())?;
                preset_add(&path, a)
            }
        },
    }
}

fn preset_list() -> Result<()> {
    use presets::PRESETS;
    println!("{:<14} {:<54} ENV", "NAME", "DESCRIPTION");
    for p in PRESETS {
        let env_summary = if p.required_env.is_empty() {
            String::from("—")
        } else {
            p.required_env.join(",")
        };
        println!(
            "{:<14} {:<54} {}",
            p.name,
            summary(p.description, 54),
            env_summary
        );
    }
    println!();
    println!("Add one with: jekko mcp preset add <name> [--as <server-name>]");
    Ok(())
}

fn preset_add(config_path: &std::path::Path, args: &McpPresetAddArgs) -> Result<()> {
    let preset = presets::find_preset(&args.name).ok_or_else(|| {
        anyhow!(
            "unknown preset `{}`; run `jekko mcp preset list` to see available presets",
            args.name
        )
    })?;
    let server_name = args
        .rename
        .clone()
        .unwrap_or_else(|| preset.name.to_string());
    validate_server_name(&server_name).map_err(map_mcp_err)?;

    // Warn (don't fail) when the user is missing required env vars: the
    // server itself will reject without them, but we let the user attach
    // first and supply the env later.
    let missing: Vec<&&str> = preset
        .required_env
        .iter()
        .filter(|var| std::env::var(var).is_err())
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "note: preset `{}` references env vars not currently set: {}",
            preset.name,
            missing.iter().map(|s| **s).collect::<Vec<_>>().join(", ")
        );
        eprintln!(
            "      (the config stores `${{VAR}}` placeholders; set the values before `jekko mcp status {}`)",
            server_name
        );
    }

    let server_cfg = preset.to_server_config();
    write_server_entry(config_path, &server_name, &server_cfg, args.force).map_err(map_mcp_err)?;
    println!(
        "attached preset `{}` as mcp server `{}` to {}",
        preset.name,
        server_name,
        config_path.display()
    );
    println!("  homepage: {}", preset.homepage);
    Ok(())
}

fn resolve_config_path(override_path: Option<&std::path::Path>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p.to_path_buf());
    }
    default_mcp_config_path().ok_or_else(|| {
        anyhow!(
            "neither JEKKO_HOME nor HOME is set; pass --config <path> or export one of those vars"
        )
    })
}

fn list(config_path: &std::path::Path) -> Result<()> {
    let cfg = load_or_empty(config_path).map_err(map_mcp_err)?;
    if cfg.servers.is_empty() {
        println!("no mcp servers configured in {}", config_path.display());
        return Ok(());
    }
    println!("{:<24} {:<10} COMMAND", "NAME", "TRANSPORT");
    for (name, server) in &cfg.servers {
        let cmd_summary = if server.args.is_empty() {
            server.command.clone()
        } else {
            format!("{} {}", server.command, server.args.join(" "))
        };
        println!("{:<24} {:<10} {}", name, server.transport, cmd_summary);
    }
    Ok(())
}

fn attach(config_path: &std::path::Path, args: &McpAttachArgs) -> Result<()> {
    if args.transport != "stdio" && args.transport != "sse" {
        bail!(
            "unknown transport `{}`; expected `stdio` or `sse`",
            args.transport
        );
    }
    if args.transport == "sse" && !args.args.is_empty() {
        bail!("sse transport does not accept positional args after `--`");
    }
    if args.transport == "stdio" && args.target.is_empty() {
        bail!("stdio transport requires a non-empty command");
    }

    validate_server_name(&args.name).map_err(map_mcp_err)?;

    let cfg = McpServerConfig {
        transport: args.transport.clone(),
        command: args.target.clone(),
        args: args.args.clone(),
        env: Default::default(),
        timeouts: Default::default(),
    };
    write_server_entry(config_path, &args.name, &cfg, args.force).map_err(map_mcp_err)?;
    println!(
        "attached mcp server `{}` ({}) to {}",
        args.name,
        args.transport,
        config_path.display()
    );
    Ok(())
}

fn detach(config_path: &std::path::Path, name: &str) -> Result<()> {
    validate_server_name(name).map_err(map_mcp_err)?;
    remove_server_entry(config_path, name).map_err(map_mcp_err)?;
    println!(
        "detached mcp server `{}` from {}",
        name,
        config_path.display()
    );
    Ok(())
}

async fn status(
    config_path: &std::path::Path,
    name: &str,
    timeout_override: Option<u64>,
) -> Result<()> {
    validate_server_name(name).map_err(map_mcp_err)?;
    let cfg = load_or_empty(config_path).map_err(map_mcp_err)?;
    let server = cfg
        .servers
        .get(name)
        .ok_or_else(|| map_mcp_err(McpError::UnknownServer(name.to_string())))?;
    if server.transport != "stdio" {
        bail!(
            "mcp status only supports stdio transport in v1; `{}` uses `{}`",
            name,
            server.transport
        );
    }
    let timeout_secs = timeout_override.unwrap_or_else(|| server.timeout_secs("default"));
    let mut client = StdioClient::spawn(name, server)
        .await
        .map_err(map_mcp_err)?;
    let init_res = client
        .initialize(timeout_secs)
        .await
        .map_err(|e| anyhow!("mcp `{}` initialize failed: {}", name, e))?;
    let tools = client
        .list_tools(timeout_secs)
        .await
        .map_err(|e| anyhow!("mcp `{}` tools/list failed: {}", name, e))?;
    println!("mcp server `{}`: OK", name);
    if let Some(server_info) = init_res.get("serverInfo") {
        if let Some(s_name) = server_info.get("name").and_then(|v| v.as_str()) {
            print!("  server: {}", s_name);
            if let Some(v) = server_info.get("version").and_then(|v| v.as_str()) {
                print!(" ({})", v);
            }
            println!();
        }
    }
    if let Some(proto) = init_res.get("protocolVersion").and_then(|v| v.as_str()) {
        println!("  protocol: {}", proto);
    }
    println!("  tools: {}", tools.len());
    for t in tools.iter().take(20) {
        match &t.description {
            Some(d) => println!("    - {}: {}", t.name, summary(d, 80)),
            None => println!("    - {}", t.name),
        }
    }
    if tools.len() > 20 {
        println!("    ... and {} more", tools.len() - 20);
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), client.shutdown()).await;
    Ok(())
}

fn summary(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

fn map_mcp_err(e: McpError) -> anyhow::Error {
    anyhow!("{e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_truncates_with_ellipsis() {
        assert_eq!(summary("short", 10), "short");
        let s = summary(&"abcdefghij".repeat(3), 10);
        assert!(s.ends_with('…'));
        assert_eq!(s.chars().count(), 11);
    }
}
