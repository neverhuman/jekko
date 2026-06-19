//! `jekko fetch` — a deterministic, jekko-owned HTTP GET over the `webfetch` tool.
//!
//! This exposes jekko's `webfetch` capability as a non-agentic CLI primitive so a
//! driver (e.g. the metavision research step) can fetch no-key research APIs
//! (OpenAlex / Crossref / arXiv / PatentsView) reliably and deterministically —
//! jekko owns the fetch, with no dependence on flaky model tool-call synthesis.

use anyhow::{Context, Result};
use clap::Args;

use jekko_runtime::tool::webfetch::{fetch_url, WebFetchInput};

use crate::cli::GlobalOpts;

/// `jekko fetch` arguments.
#[derive(Args, Debug)]
pub struct FetchArgs {
    /// URL to GET (http/https only).
    #[arg(value_name = "URL")]
    pub url: String,

    /// Per-request timeout in ms (hard ceiling 30000).
    #[arg(long = "timeout-ms", default_value_t = 25_000)]
    pub timeout_ms: u64,

    /// Max body bytes to keep.
    #[arg(long = "max-bytes", default_value_t = 2_000_000)]
    pub max_bytes: u64,

    /// Print only the response body (default prints a JSON envelope with status).
    #[arg(long = "body-only", action = clap::ArgAction::SetTrue)]
    pub body_only: bool,
}

pub fn run(_global: &GlobalOpts, args: &FetchArgs) -> Result<()> {
    let input = WebFetchInput {
        url: args.url.clone(),
        method: Some("GET".to_string()),
        headers: None,
        timeout_ms: Some(args.timeout_ms),
        max_bytes: Some(args.max_bytes),
    };

    let resp = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?
        .block_on(fetch_url(&input))
        .with_context(|| format!("fetch {}", args.url))?;

    if args.body_only {
        println!("{}", resp.body);
    } else {
        // JSON envelope so callers can check status/truncation, then parse `body`.
        let v = serde_json::to_value(&resp).context("serialize response")?;
        println!("{}", serde_json::to_string(&v).context("encode envelope")?);
    }
    Ok(())
}
