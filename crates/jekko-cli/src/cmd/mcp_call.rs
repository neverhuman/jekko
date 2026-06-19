//! `jekko mcp-call` — call a tool on a registered MCP server (JSON-RPC `tools/call`).
//!
//! This is the jekko-native, no-subprocess bridge to MCP servers such as **jailgun**
//! (ChatGPT-Pro). godmode uses it to evolve a metavision version *through jekko*:
//! `jekko mcp-call jailgun jailgun.run --json '<JailgunAgentRunRequest>'`, then polls
//! `jailgun.run_status`. The command is generic — the caller supplies the tool
//! arguments; this just frames + transports the JSON-RPC call and prints the response.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::{json, Value};

use crate::cli::GlobalOpts;

/// `jekko mcp-call` arguments.
#[derive(Args, Debug)]
pub struct McpCallArgs {
    /// MCP server label (also resolves `--url` from `JEKKO_MCP_<SERVER>_URL` if `--url` omitted).
    #[arg(value_name = "SERVER")]
    pub server: String,

    /// Tool to call, e.g. `jailgun.run`.
    #[arg(value_name = "TOOL")]
    pub tool: String,

    /// MCP server JSON-RPC endpoint URL (the `/mcp` endpoint). Overrides env.
    #[arg(long)]
    pub url: Option<String>,

    /// Full tool arguments as a JSON object (string args from `--arg` are merged in).
    #[arg(long)]
    pub json: Option<String>,

    /// Append a string argument `key=value` (repeatable).
    #[arg(long = "arg", value_name = "K=V")]
    pub args: Vec<String>,

    /// HTTP timeout in ms (jailgun runs can be slow).
    #[arg(long = "timeout-ms", default_value_t = 600_000)]
    pub timeout_ms: u64,

    /// Extra HTTP header `Key:Value` (repeatable) — e.g. `x-jailgun-token:...`.
    #[arg(long = "header", value_name = "K:V")]
    pub headers: Vec<String>,

    /// Print only `result.structuredContent` (default prints the whole JSON-RPC response).
    #[arg(long = "structured", action = clap::ArgAction::SetTrue)]
    pub structured: bool,
}

fn resolve_url(args: &McpCallArgs) -> Result<String> {
    if let Some(u) = &args.url {
        return Ok(u.clone());
    }
    let key = format!(
        "JEKKO_MCP_{}_URL",
        args.server.to_uppercase().replace('-', "_")
    );
    if let Ok(u) = std::env::var(&key) {
        return Ok(u);
    }
    if let Ok(u) = std::env::var("JEKKO_MCP_URL") {
        return Ok(u);
    }
    anyhow::bail!("no MCP url for '{}': pass --url or set {key}", args.server)
}

pub fn run(_global: &GlobalOpts, args: &McpCallArgs) -> Result<()> {
    let url = resolve_url(args)?;

    let mut arguments: Value = match &args.json {
        Some(s) => serde_json::from_str(s).context("parse --json arguments")?,
        None => json!({}),
    };
    if !arguments.is_object() {
        anyhow::bail!("--json must be a JSON object");
    }
    {
        let obj = arguments.as_object_mut().expect("checked object");
        for kv in &args.args {
            let (k, v) = kv
                .split_once('=')
                .with_context(|| format!("--arg must be key=value (got `{kv}`)"))?;
            obj.insert(k.to_string(), Value::String(v.to_string()));
        }
    }

    let envelope = json!({
        "jsonrpc": "2.0",
        "id": format!("{}-{}", args.server, args.tool),
        "method": "tools/call",
        "params": { "name": args.tool, "arguments": arguments },
    });

    let timeout = Duration::from_millis(args.timeout_ms);
    let url_for_err = url.clone();
    let headers_vec = args.headers.clone();
    let body: Value = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?
        .block_on(async move {
            let client = reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .context("build http client")?;
            let mut req = client.post(&url).json(&envelope);
            for h in &headers_vec {
                let (k, v) = h
                    .split_once(':')
                    .with_context(|| format!("--header must be Key:Value (got `{h}`)"))?;
                req = req.header(k.trim().to_string(), v.trim().to_string());
            }
            let resp = req
                .send()
                .await
                .with_context(|| format!("POST {url}"))?;
            let v: Value = resp.json().await.context("parse JSON-RPC response")?;
            Ok::<Value, anyhow::Error>(v)
        })
        .with_context(|| format!("mcp-call {} {}", args.server, args.tool))?;

    if let Some(err) = body.get("error") {
        anyhow::bail!("MCP error from {} ({}): {err}", args.tool, url_for_err);
    }

    let out = if args.structured {
        body.get("result")
            .and_then(|r| r.get("structuredContent"))
            .cloned()
            .unwrap_or(body)
    } else {
        body
    };
    println!("{}", serde_json::to_string(&out).context("encode output")?);
    Ok(())
}
