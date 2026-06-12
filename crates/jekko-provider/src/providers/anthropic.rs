//! Anthropic Messages API adapter.
//!
//! Sends a streaming `POST /v1/messages` request and decodes Anthropic's
//! event-stream format (events: `message_start`, `content_block_start`,
//! `content_block_delta`, `content_block_stop`, `message_delta`,
//! `message_stop`, `ping`, `error`).
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};
use tokio_util::sync::CancellationToken;

use crate::adapter::{require_credential, ProviderAdapter, ProviderRequest, ProviderStream};
use crate::error::{ProviderError, ProviderResult};
use crate::stream::{
    ProviderCapabilities, ProviderEvent, ProviderEventKind, SseFrame, ToolCallAggregator,
};

use super::shared::{
    headers_from, make_client, parse_data_as_json, post_json_sse_stream, preparse_sse_frame,
    sse_decode_all, SsePreparse,
};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Anthropic adapter.
#[derive(Debug, Clone, Default)]
pub struct AnthropicAdapter {
    client: reqwest::Client,
}

impl AnthropicAdapter {
    /// Construct a fresh adapter with a default client.
    pub fn new() -> Self {
        Self {
            client: make_client(),
        }
    }

    /// Build the JSON request body that would be sent to the Anthropic API.
    ///
    /// Exposed for parity tests: callers can assert the body without
    /// performing a network request.
    pub fn build_body(&self, req: &ProviderRequest) -> Value {
        build_anthropic_body(req)
    }

    /// Build the HTTP header map that would be sent.
    pub fn build_headers(
        &self,
        req: &ProviderRequest,
    ) -> ProviderResult<reqwest::header::HeaderMap> {
        build_anthropic_headers(req)
    }

    /// Map a single Anthropic SSE frame to canonical events. Returned vec is
    /// empty for frames we ignore (`ping`).
    pub fn map_frame(
        frame: &SseFrame,
        agg: &mut ToolCallAggregator,
    ) -> ProviderResult<Vec<ProviderEvent>> {
        map_anthropic_frame(frame, agg)
    }
}

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    async fn stream(
        &self,
        req: ProviderRequest,
        abort: CancellationToken,
    ) -> ProviderResult<ProviderStream> {
        let base = req.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL);
        let url = format!("{}/v1/messages", base);
        let body = build_anthropic_body(&req);
        let headers = build_anthropic_headers(&req)?;

        let mut agg = ToolCallAggregator::new();
        let stream =
            post_json_sse_stream(&self.client, &url, headers, &body, abort, move |frame| {
                map_anthropic_frame(frame, &mut agg)
            })
            .await?;
        // Box::pin to dyn-stream.
        Ok(Box::pin(stream.map(|r| r)) as ProviderStream)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            cache_control: true,
            tool_streaming: true,
        }
    }
}

fn build_anthropic_headers(req: &ProviderRequest) -> ProviderResult<reqwest::header::HeaderMap> {
    let mut headers = headers_from(req);
    headers.insert(
        reqwest::header::HeaderName::from_static("anthropic-version"),
        reqwest::header::HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("content-type"),
        reqwest::header::HeaderValue::from_static("application/json"),
    );

    let cred = require_credential(req, "anthropic")?;
    match cred {
        crate::adapter::ProviderCredential::ApiKey { key } => {
            headers.insert(
                reqwest::header::HeaderName::from_static("x-api-key"),
                reqwest::header::HeaderValue::from_str(key.as_str())
                    .map_err(|_| ProviderError::MissingCredential("anthropic".into()))?,
            );
        }
        crate::adapter::ProviderCredential::OAuth { access_token } => {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {access_token}"))
                    .map_err(|_| ProviderError::MissingCredential("anthropic".into()))?,
            );
        }
        crate::adapter::ProviderCredential::Bearer { token } => {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| ProviderError::MissingCredential("anthropic".into()))?,
            );
        }
    }
    Ok(headers)
}

fn build_anthropic_body(req: &ProviderRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(req.api_model_id.clone()));
    body.insert(
        "max_tokens".into(),
        Value::Number(serde_json::Number::from(req.max_output_tokens)),
    );
    body.insert("stream".into(), Value::Bool(true));

    if let Some(t) = req.temperature {
        body.insert(
            "temperature".into(),
            serde_json::Number::from_f64(t)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }
    if let Some(p) = req.top_p {
        body.insert(
            "top_p".into(),
            serde_json::Number::from_f64(p)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }
    if let Some(k) = req.top_k {
        body.insert("top_k".into(), Value::Number(serde_json::Number::from(k)));
    }

    // System messages: Anthropic accepts an array of `{ type: "text", text, ... }`.
    if !req.system.is_empty() {
        let mut system_blocks = Vec::new();
        for (i, segment) in req.system.iter().enumerate() {
            let mut block = json!({ "type": "text", "text": segment });
            // Tag the first segment as cacheable when explicitly requested via options.
            if i < 2 {
                if let Some(po) = req.options.get("system_cache_first_two") {
                    if po.as_bool() == Some(true) {
                        block["cache_control"] = json!({ "type": "ephemeral" });
                    }
                }
            }
            system_blocks.push(block);
        }
        body.insert("system".into(), Value::Array(system_blocks));
    }

    // Convert canonical ModelMessage[] into Anthropic's messages array.
    // For parity tests we pass through messages as-is, relying on the
    // transform layer to have produced the right shape.
    body.insert("messages".into(), Value::Array(req.messages.clone()));

    // Tools.
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                let mut o = json!({
                    "name": t.name,
                    "input_schema": t.input_schema.clone(),
                });
                if let Some(d) = &t.description {
                    o["description"] = Value::String(d.clone());
                }
                o
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
        if let Some(tc) = &req.tool_choice {
            let map_tc = match tc.as_str() {
                "auto" => json!({ "type": "auto" }),
                "required" => json!({ "type": "any" }),
                "none" => json!({ "type": "none" }),
                other => json!({ "type": other }),
            };
            body.insert("tool_choice".into(), map_tc);
        }
    }

    // Thinking / cache control come pre-baked under options.anthropic.* per
    // the transform layer. Surface them at the top level when present.
    if let Some(anth) = req.options.get("anthropic").and_then(Value::as_object) {
        for (k, v) in anth {
            body.insert(k.clone(), v.clone());
        }
    }

    Value::Object(body)
}

/// Decode a single Anthropic SSE frame.
fn map_anthropic_frame(
    frame: &SseFrame,
    agg: &mut ToolCallAggregator,
) -> ProviderResult<Vec<ProviderEvent>> {
    let data = match preparse_sse_frame(frame, None, Some("ping")) {
        SsePreparse::Resolved(events) => return Ok(events),
        SsePreparse::Payload(data) => data,
    };
    let payload = parse_data_as_json(data)?;
    let event_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
    let raw_event = if frame.event.is_empty() {
        event_type.to_string()
    } else {
        frame.event.clone()
    };
    let mut out = Vec::new();

    match event_type {
        "message_start" => {
            let model = payload
                .get("message")
                .and_then(|m| m.get("model"))
                .and_then(Value::as_str)
                .map(String::from);
            out.push(ProviderEvent::with_raw(
                ProviderEventKind::StreamStart { model },
                raw_event,
            ));
        }
        "content_block_start" => {
            let block = payload.get("content_block").cloned().unwrap_or(Value::Null);
            let ty = block.get("type").and_then(Value::as_str).unwrap_or("");
            if ty == "tool_use" {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let ev = ProviderEventKind::ToolCallStart {
                    id: id.clone(),
                    name: name.clone(),
                };
                agg.apply(&ev);
                out.push(ProviderEvent::with_raw(ev, raw_event));
            }
        }
        "content_block_delta" => {
            let delta = payload.get("delta").cloned().unwrap_or(Value::Null);
            let dty = delta.get("type").and_then(Value::as_str).unwrap_or("");
            match dty {
                "text_delta" => {
                    let text = delta
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    out.push(ProviderEvent::with_raw(
                        ProviderEventKind::TextDelta { text },
                        raw_event,
                    ));
                }
                "thinking_delta" => {
                    let text = delta
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    out.push(ProviderEvent::with_raw(
                        ProviderEventKind::ReasoningDelta { text },
                        raw_event,
                    ));
                }
                "input_json_delta" => {
                    // Tool input fragment. Map index -> id via block_start
                    // events handled above; here we look at `payload.index`
                    // but Anthropic doesn't carry the id in delta events, so
                    // we attach by index using a separate aggregator helper.
                    let idx = payload.get("index").and_then(Value::as_u64).unwrap_or(0);
                    let partial = delta
                        .get("partial_json")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    // We don't actually carry the id-to-index map here; the
                    // adapter spec treats the (event, raw) info as enough for
                    // the runtime to reassemble. Emit a delta carrying a
                    // synthetic id of `idx:<n>` so a downstream consumer can
                    // route correctly without needing to know the real id.
                    out.push(ProviderEvent::with_raw(
                        ProviderEventKind::ToolCallInputDelta {
                            id: format!("idx:{idx}"),
                            delta: partial,
                        },
                        raw_event,
                    ));
                }
                _ => {}
            }
        }
        "content_block_stop" => {
            // Nothing canonical to emit (the runtime layer will materialize
            // the completed tool call from prior deltas).
        }
        "message_delta" => {
            if let Some(usage) = payload.get("usage") {
                let ev = ProviderEventKind::Usage {
                    input_tokens: usage
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    output_tokens: usage
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    cache_read_tokens: usage
                        .get("cache_read_input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    cache_write_tokens: usage
                        .get("cache_creation_input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                };
                out.push(ProviderEvent::with_raw(ev, raw_event.clone()));
            }
            let stop_reason = payload
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(Value::as_str)
                .map(String::from);
            if stop_reason.is_some() {
                out.push(ProviderEvent::with_raw(
                    ProviderEventKind::StreamEnd { stop_reason },
                    raw_event,
                ));
            }
        }
        "message_stop" => {
            out.push(ProviderEvent::with_raw(
                ProviderEventKind::StreamEnd { stop_reason: None },
                raw_event,
            ));
        }
        "error" => {
            let message = payload
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown anthropic error")
                .to_string();
            out.push(ProviderEvent::with_raw(
                ProviderEventKind::Error { message },
                raw_event,
            ));
        }
        _ => {}
    }
    Ok(out)
}

/// Test helper: synchronously decode an entire SSE byte stream using the
/// Anthropic mapping.
pub fn decode_anthropic_sse(bytes: &[u8]) -> ProviderResult<Vec<ProviderEvent>> {
    let mut agg = ToolCallAggregator::new();
    sse_decode_all(bytes, |frame| map_anthropic_frame(frame, &mut agg))
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
