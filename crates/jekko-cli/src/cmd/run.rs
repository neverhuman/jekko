//! `jekko run <prompt>` — non-interactive run.
//!
//! Mirrors `packages/jekko/src/cli/cmd/run.ts`. The Rust implementation now
//! routes through the runtime boundary and persists the prompt/session
//! bookkeeping that the former JS launcher wrapped around every run.

use anyhow::{bail, Context, Result};
use clap::Args;
use jekko_runtime::{RunRequest, Runtime as JekkoRuntime};

use crate::cli::GlobalOpts;

/// `jekko run` arguments. Match the TS yargs surface.
#[derive(Args, Debug, Default)]
pub struct RunArgs {
    /// Prompt to send to the agent. When omitted, stdin is read.
    ///
    /// Example: `jekko run "refactor the cache layer"`.
    pub prompt: Option<String>,

    /// Continue the last session.
    #[arg(long = "continue", short = 'c')]
    pub r#continue: bool,

    /// Open a specific session by id.
    #[arg(short = 's', value_name = "SESSION_ID")]
    pub session: Option<String>,

    /// Provider to use (e.g. `anthropic`, `openai`).
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Model to use (e.g. `claude-opus-4-7`).
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Agent identifier to run (e.g. `build`, `plan`).
    #[arg(long, value_name = "AGENT")]
    pub agent: Option<String>,

    /// Override the working directory for this single run.
    #[arg(long = "cwd", value_name = "PATH")]
    pub cwd: Option<std::path::PathBuf>,

    /// Run without persisting the session.
    #[arg(long)]
    pub ephemeral: bool,

    /// Print the agent's response as a single line of JSON.
    #[arg(long)]
    pub json: bool,
}

/// Execute a one-shot prompt run.
pub fn run(_global: &GlobalOpts, args: &RunArgs) -> Result<()> {
    let prompt = match args.prompt.as_deref() {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => read_stdin_prompt()?,
    };

    if prompt.trim().is_empty() {
        bail!("no prompt provided");
    }

    let cwd = match args.cwd.clone() {
        Some(cwd) => cwd,
        None => std::env::current_dir().context("resolving working directory")?,
    };
    let runtime = JekkoRuntime::new();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(runtime.run_oneshot(RunRequest {
            prompt,
            cwd,
            agent: args.agent.clone(),
            provider: args.provider.clone(),
            model: args.model.clone(),
            ephemeral: args.ephemeral,
        }))?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        if !result.success {
            bail!(result
                .error
                .clone()
                .unwrap_or_else(|| "run failed".to_string()));
        }
    } else if let Some(text) = result.assistant_text.as_deref() {
        println!("{text}");
        if let Some(session) = &result.session {
            eprintln!(
                "jekko run: session {} ({}) accepted",
                session.id, session.title
            );
        }
        if let Some(provider) = result.provider_id.as_deref() {
            eprintln!("  provider: {provider}");
        }
        if let Some(model) = result.model_id.as_deref() {
            eprintln!("  model: {model}");
        }
        if !result.success {
            bail!(result
                .error
                .clone()
                .unwrap_or_else(|| "run failed".to_string()));
        }
    } else if let Some(session) = &result.session {
        eprintln!(
            "jekko run: session {} ({}) accepted",
            session.id, session.title
        );
        if let Some(provider) = args.provider.as_deref() {
            eprintln!("  provider: {provider}");
        }
        if let Some(model) = args.model.as_deref() {
            eprintln!("  model: {model}");
        }
        if !result.success {
            bail!(result
                .error
                .clone()
                .unwrap_or_else(|| "run failed".to_string()));
        }
    } else {
        eprintln!("jekko run: ephemeral prompt accepted");
        if !result.success {
            bail!(result
                .error
                .clone()
                .unwrap_or_else(|| "run failed".to_string()));
        }
    }
    Ok(())
}

fn read_stdin_prompt() -> Result<String> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    Ok(s)
}
