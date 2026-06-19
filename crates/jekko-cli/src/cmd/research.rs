//! `jekko research` — deterministic, jekko-native online research harness.
//!
//! Fans out each aspect's queries to no-key providers (OpenAlex / Crossref /
//! PatentsView) via jekko's `webfetch` (`fetch_url`), parses results, and emits
//! contract-compliant evidence receipts to a JSONL file. There is NO model in the
//! loop and NO raw third-party HTTP outside jekko's own tool — jekko owns the
//! fetch, so research is exhaustive, reproducible, and receipt-audited.

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use jekko_runtime::tool::webfetch::{fetch_url, WebFetchInput};

use crate::cli::GlobalOpts;

/// `jekko research` arguments.
#[derive(Args, Debug)]
pub struct ResearchArgs {
    /// Path to aspects.json (`{aspects:[{id,title,queries[]}]}`).
    #[arg(long, value_name = "PATH")]
    pub aspects: PathBuf,

    /// Output JSONL receipts path (e.g. `out/research_evidence.jsonl`).
    #[arg(long, value_name = "PATH")]
    pub out: PathBuf,

    /// Providers (comma-separated). Keyless: `openalex,crossref,arxiv`.
    /// (`patentsview` is supported but now needs an API key, so it's off by default.)
    #[arg(long, default_value = "openalex,crossref,arxiv")]
    pub providers: String,

    /// Queries per aspect to issue.
    #[arg(long = "queries-per-aspect", default_value_t = 2)]
    pub queries_per_aspect: usize,

    /// Results to keep per (provider, query).
    #[arg(long = "per-query", default_value_t = 3)]
    pub per_query: usize,

    /// Contact email for OpenAlex's polite pool.
    #[arg(long, default_value = "metavision@amphora.local")]
    pub mailto: String,

    /// Limit number of aspects (0 = all) — for fast testing.
    #[arg(long = "max-aspects", default_value_t = 0)]
    pub max_aspects: usize,
}

pub fn run(_global: &GlobalOpts, args: &ResearchArgs) -> Result<()> {
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(&args.aspects)
            .with_context(|| format!("read aspects {}", args.aspects.display()))?,
    )
    .context("parse aspects.json")?;
    let mut aspects = doc
        .get("aspects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if args.max_aspects > 0 && aspects.len() > args.max_aspects {
        aspects.truncate(args.max_aspects);
    }
    let providers: Vec<String> = args
        .providers
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut receipts: Vec<Value> = Vec::new();
    let mut providers_hit: BTreeSet<String> = BTreeSet::new();
    let mut ok = 0usize;

    for aspect in &aspects {
        let aid = aspect.get("id").and_then(Value::as_str).unwrap_or("aspect");
        let queries = aspect
            .get("queries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for q in queries.iter().take(args.queries_per_aspect) {
            let query = q.as_str().unwrap_or("");
            if query.is_empty() {
                continue;
            }
            for provider in &providers {
                let url = match build_url(provider, query, args.per_query, &args.mailto) {
                    Some(u) => u,
                    None => continue,
                };
                let input = WebFetchInput {
                    url: url.clone(),
                    method: Some("GET".to_string()),
                    headers: None,
                    timeout_ms: Some(25_000),
                    max_bytes: Some(2_000_000),
                };
                match rt.block_on(fetch_url(&input)) {
                    Ok(resp) if (200..300).contains(&resp.status) => {
                        for item in parse_items(provider, &resp.body) {
                            if let Some(r) = item_to_receipt(provider, &item, aid) {
                                let key = format!(
                                    "{provider}|{}",
                                    r.get("url").and_then(Value::as_str).unwrap_or("")
                                );
                                if seen.insert(key) {
                                    providers_hit.insert(canonical_provider(provider));
                                    ok += 1;
                                    receipts.push(r);
                                }
                            }
                        }
                    }
                    Ok(resp) => receipts.push(skip_receipt(
                        provider,
                        aid,
                        &format!("http_{}", resp.status),
                    )),
                    Err(e) => receipts.push(skip_receipt(provider, aid, &e.to_string())),
                }
            }
        }
    }

    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut buf = String::new();
    for r in &receipts {
        buf.push_str(&serde_json::to_string(r).context("encode receipt")?);
        buf.push('\n');
    }
    std::fs::write(&args.out, buf).with_context(|| format!("write {}", args.out.display()))?;

    println!(
        "jekko research: {} receipts ({ok} ok) over providers {:?} from {} aspect(s) -> {}",
        receipts.len(),
        providers_hit,
        aspects.len(),
        args.out.display()
    );
    Ok(())
}

/// Percent-encode for a URL query component (RFC3986 unreserved kept).
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn build_url(provider: &str, query: &str, n: usize, mailto: &str) -> Option<String> {
    let q = pct(query);
    match provider {
        "openalex" => Some(format!(
            "https://api.openalex.org/works?search={q}&per_page={n}&mailto={}",
            pct(mailto)
        )),
        "crossref" => Some(format!("https://api.crossref.org/works?query={q}&rows={n}")),
        "arxiv" => Some(format!(
            "http://export.arxiv.org/api/query?search_query=all:{q}&max_results={n}"
        )),
        "patentsview" => {
            // Both `q` and `o` are JSON documents, URL-encoded.
            let title = query.replace('"', " ");
            let q_json = pct(&format!("{{\"_text_any\":{{\"patent_title\":\"{title}\"}}}}"));
            let o_json = pct(&format!("{{\"size\":{n}}}"));
            Some(format!(
                "https://search.patentsview.org/api/v1/patent/?q={q_json}&o={o_json}"
            ))
        }
        _ => None,
    }
}

fn parse_items(provider: &str, body: &str) -> Vec<Value> {
    if provider == "arxiv" {
        return parse_arxiv(body);
    }
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match provider {
        "openalex" => v.get("results").and_then(Value::as_array),
        "crossref" => v
            .get("message")
            .and_then(|m| m.get("items"))
            .and_then(Value::as_array),
        "patentsview" => v.get("patents").and_then(Value::as_array),
        _ => None,
    };
    arr.cloned().unwrap_or_default()
}

/// Parse arXiv Atom XML into `{title, arxiv_url}` items (one per `<entry>`).
fn parse_arxiv(body: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for entry in body.split("<entry>").skip(1) {
        let entry = entry.split("</entry>").next().unwrap_or("");
        let id = extract_tag(entry, "id").unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        let title = normalize_ws(&extract_tag(entry, "title").unwrap_or_default());
        out.push(json!({ "title": title, "arxiv_url": id }));
    }
    out
}

fn extract_tag(s: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open)? + open.len();
    let rest = &s[start..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim().to_string())
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Canonical provider name for the fitness coverage set {openalex,crossref,arxiv,patent,standards}.
fn canonical_provider(p: &str) -> String {
    match p {
        "patentsview" => "patent",
        other => other,
    }
    .to_string()
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn item_to_receipt(provider: &str, item: &Value, aspect_id: &str) -> Option<Value> {
    let (kind, tier) = match provider {
        "openalex" | "crossref" => ("source", "primary"),
        "arxiv" => ("source", "secondary"),
        "patentsview" => ("patent", "patent"),
        _ => return None,
    };
    let (title, url, doi, patent_id) = match provider {
        "openalex" => {
            let title = item.get("title").and_then(Value::as_str).unwrap_or("").to_string();
            let doi = item.get("doi").and_then(Value::as_str).map(String::from);
            let url = doi
                .clone()
                .or_else(|| item.get("id").and_then(Value::as_str).map(String::from))
                .unwrap_or_default();
            (title, url, doi, None)
        }
        "crossref" => {
            let title = item
                .get("title")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let doi = item.get("DOI").and_then(Value::as_str).map(String::from);
            let url = doi
                .as_ref()
                .map(|d| format!("https://doi.org/{d}"))
                .unwrap_or_default();
            (title, url, doi, None)
        }
        "arxiv" => {
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let url = item
                .get("arxiv_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            (title, url, None, None)
        }
        "patentsview" => {
            let pid = item.get("patent_id").and_then(Value::as_str).map(String::from);
            let title = item
                .get("patent_title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let url = pid
                .as_ref()
                .map(|p| format!("https://patents.google.com/patent/US{p}"))
                .unwrap_or_default();
            (title, url, None, pid)
        }
        _ => return None,
    };
    // Contract: never emit placeholder-domain citations.
    if url.is_empty() || url.to_lowercase().contains("example.com") {
        return None;
    }
    let canon = serde_json::to_string(item).unwrap_or_default();
    let id: String = sha256_hex(&format!("{provider}:{url}")).chars().take(16).collect();
    Some(json!({
        "id": id,
        "kind": kind,
        "provider": canonical_provider(provider),
        "title": title.chars().take(180).collect::<String>(),
        "url": url,
        "doi": doi,
        "patent_id": patent_id,
        "tier": tier,
        "sha256": format!("sha256:{}", canon_hash(&canon)),
        "claim_ids": [aspect_id],
        "status": "ok",
    }))
}

fn canon_hash(canon: &str) -> String {
    sha256_hex(canon).chars().take(32).collect()
}

fn skip_receipt(provider: &str, aspect_id: &str, reason: &str) -> Value {
    let id: String = sha256_hex(&format!("skip:{provider}:{aspect_id}:{reason}"))
        .chars()
        .take(16)
        .collect();
    json!({
        "id": id,
        "kind": "skip_receipt",
        "provider": canonical_provider(provider),
        "status": "skipped",
        "reason": reason,
        "claim_ids": [aspect_id],
    })
}
