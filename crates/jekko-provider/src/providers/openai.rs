//! OpenAI Chat Completions API adapter.
//!
//! Targets the chat-completions endpoint (`POST /v1/chat/completions`) with
//! `stream: true`. SSE frames are JSON payloads of the OpenAI delta format.
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::adapter::{ProviderAdapter, ProviderRequest, ProviderStream};
use crate::error::ProviderResult;
use crate::stream::{ProviderCapabilities, ProviderEvent, ProviderEventKind, SseFrame};

use super::shared::{
    build_openai_style_body, build_openai_style_headers, make_client, parse_data_as_json,
    post_json_sse_stream, preparse_sse_frame, sse_decode_all, SsePreparse,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com";

/// OpenAI adapter.
#[derive(Debug, Clone, Default)]
pub struct OpenAiAdapter {
    client: reqwest::Client,
}

impl OpenAiAdapter {
    /// Construct a new adapter.
    pub fn new() -> Self {
        Self {
            client: make_client(),
        }
    }

    /// Build the JSON body that would be POSTed.
    pub fn build_body(&self, req: &ProviderRequest) -> Value {
        build_openai_style_body(req)
    }

    /// Build the request headers.
    pub fn build_headers(
        &self,
        req: &ProviderRequest,
    ) -> ProviderResult<reqwest::header::HeaderMap> {
        build_openai_style_headers(req, "openai")
    }

    /// Map a single OpenAI SSE frame to canonical events.
    pub fn map_frame(frame: &SseFrame) -> ProviderResult<Vec<ProviderEvent>> {
        map_openai_frame(frame)
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiAdapter {
    async fn stream(
        &self,
        req: ProviderRequest,
        abort: CancellationToken,
    ) -> ProviderResult<ProviderStream> {
        let base = req.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL);
        let url = format!("{}/v1/chat/completions", base);
        let body = build_openai_style_body(&req);
        let headers = build_openai_style_headers(&req, "openai")?;

        let mut state = OpenAiStreamState::new();
        let stream =
            post_json_sse_stream(&self.client, &url, headers, &body, abort, move |frame| {
                map_openai_frame_stateful(frame, &mut state)
            })
            .await?;
        Ok(Box::pin(stream.map(|r| r)) as ProviderStream)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            cache_control: false,
            tool_streaming: true,
        }
    }
}

/// Per-stream state used by [`map_openai_frame_stateful`] to remember tool-call
/// ids by their index. OpenAI only includes `id` on the first chunk of a
/// given tool_call.
#[derive(Debug, Default, Clone)]
pub struct OpenAiStreamState {
    tool_call_ids: std::collections::HashMap<u64, String>,
}

impl OpenAiStreamState {
    /// Construct empty state.
    pub fn new() -> Self {
        Self::default()
    }
}

fn map_openai_frame(frame: &SseFrame) -> ProviderResult<Vec<ProviderEvent>> {
    let mut state = OpenAiStreamState::new();
    map_openai_frame_stateful(frame, &mut state)
}

/// Stateful variant of `map_openai_frame` that carries tool-call id state
/// across frames so subsequent argument deltas can be routed to the right
/// tool_call id.
pub fn map_openai_frame_stateful(
    frame: &SseFrame,
    state: &mut OpenAiStreamState,
) -> ProviderResult<Vec<ProviderEvent>> {
    if frame.event == "jnoccio-metadata" {
        let payload = parse_data_as_json(&frame.data)?;
        return Ok(vec![ProviderEvent::with_raw(
            ProviderEventKind::Metadata { metadata: payload },
            frame.event.clone(),
        )]);
    }
    let data = match preparse_sse_frame(frame, Some("[DONE]"), None) {
        SsePreparse::Resolved(events) => return Ok(events),
        SsePreparse::Payload(data) => data,
    };
    let payload = parse_data_as_json(data)?;
    Ok(decode_openai_chat_completion_chunk(&payload, state))
}

/// Decode a parsed OpenAI chat-completions chunk payload (delta + usage)
/// into the canonical provider events. Split out from
/// [`map_openai_frame_stateful`] so the SSE-level preparse and the
/// JSON-level dispatch each live in a single dedicated function.
fn decode_openai_chat_completion_chunk(
    payload: &Value,
    state: &mut OpenAiStreamState,
) -> Vec<ProviderEvent> {
    let mut out = Vec::new();

    if let Some(choices) = payload.get("choices").and_then(Value::as_array) {
        for choice in choices {
            push_choice_events(choice, state, &mut out);
        }
    }
    if let Some(usage) = payload.get("usage") {
        out.push(ProviderEvent::new(ProviderEventKind::Usage {
            input_tokens: usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            cache_read_tokens: usage
                .get("prompt_cache_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            cache_write_tokens: 0,
        }));
    }
    out
}

fn push_choice_events(choice: &Value, state: &mut OpenAiStreamState, out: &mut Vec<ProviderEvent>) {
    if let Some(delta) = choice.get("delta") {
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                out.push(ProviderEvent::new(ProviderEventKind::TextDelta {
                    text: content.to_string(),
                }));
            }
        }
        if let Some(r) = delta.get("reasoning").and_then(Value::as_str) {
            if !r.is_empty() {
                out.push(ProviderEvent::new(ProviderEventKind::ReasoningDelta {
                    text: r.to_string(),
                }));
            }
        }
        if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
            for tc in tcs {
                push_tool_call_events(tc, state, out);
            }
        }
    }
    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        out.push(ProviderEvent::new(ProviderEventKind::StreamEnd {
            stop_reason: Some(reason.to_string()),
        }));
    }
}

fn push_tool_call_events(tc: &Value, state: &mut OpenAiStreamState, out: &mut Vec<ProviderEvent>) {
    let index = tc.get("index").and_then(Value::as_u64).unwrap_or(0);
    if let Some(id) = tc.get("id").and_then(Value::as_str) {
        state.tool_call_ids.insert(index, id.to_string());
    }
    let id = match state.tool_call_ids.get(&index).cloned() {
        Some(id) => id,
        None => format!("call_idx_{index}"),
    };
    let func = tc.get("function").cloned().unwrap_or(Value::Null);
    if let Some(name) = func.get("name").and_then(Value::as_str) {
        out.push(ProviderEvent::new(ProviderEventKind::ToolCallStart {
            id: id.clone(),
            name: name.to_string(),
        }));
    }
    if let Some(args) = func.get("arguments").and_then(Value::as_str) {
        if !args.is_empty() {
            out.push(ProviderEvent::new(ProviderEventKind::ToolCallInputDelta {
                id: id.clone(),
                delta: args.to_string(),
            }));
        }
    }
}

/// Test helper: synchronously decode an entire OpenAI SSE byte stream,
/// carrying tool-call id state across frames.
pub fn decode_openai_sse(bytes: &[u8]) -> ProviderResult<Vec<ProviderEvent>> {
    let mut state = OpenAiStreamState::new();
    sse_decode_all(bytes, |frame| map_openai_frame_stateful(frame, &mut state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProviderCredential, ProviderTool};
    use crate::providers::shared::test_request_with;
    use serde_json::json;

    fn req() -> ProviderRequest {
        test_request_with(
            "openai/gpt-4",
            "gpt-4",
            ProviderCredential::Bearer {
                token: "openai-sample-key".into(),
            },
            None,
            Some(0.7),
        )
    }

    #[test]
    fn body_uses_completion_tokens() {
        let a = OpenAiAdapter::new();
        let body = a.build_body(&req());
        assert_eq!(body["model"], "gpt-4");
        assert_eq!(body["max_completion_tokens"], 4096);
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hi");
    }

    #[test]
    fn headers_use_bearer_auth() {
        let a = OpenAiAdapter::new();
        let h = a.build_headers(&req()).unwrap();
        assert_eq!(
            h.get(reqwest::header::AUTHORIZATION).unwrap(),
            "Bearer openai-sample-key"
        );
        assert!(h.get("x-api-key").is_none());
    }

    #[test]
    fn tool_definition_maps_function_shape() {
        let mut r = req();
        r.tools.push(ProviderTool {
            name: "Read".into(),
            description: Some("Read file".into()),
            input_schema: json!({ "type": "object" }),
        });
        let a = OpenAiAdapter::new();
        let body = a.build_body(&r);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "Read");
        assert_eq!(body["tools"][0]["function"]["description"], "Read file");
    }

    #[test]
    fn options_passthrough() {
        let mut r = req();
        r.options.insert("store".into(), Value::Bool(false));
        r.options
            .insert("promptCacheKey".into(), Value::String("sess-1".into()));
        let a = OpenAiAdapter::new();
        let body = a.build_body(&r);
        assert_eq!(body["store"], false);
        assert_eq!(body["prompt_cache_key"], "sess-1");
    }

    #[test]
    fn metadata_event_decodes_from_jnoccio_frame() {
        let frame = SseFrame {
            event: "jnoccio-metadata".to_string(),
            data: json!({
                "credential_user_id": "user-1",
                "winner_model_id": "provider/model"
            })
            .to_string(),
            id: None,
            retry: None,
        };
        let events = map_openai_frame_stateful(&frame, &mut OpenAiStreamState::new()).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, ProviderEventKind::Metadata { .. }));
    }
}
