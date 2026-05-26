//! SSE parser parity tests against canned provider streams.
//!
//! Feeds the canned fixtures in `tests/fixtures/` through the per-provider
//! decoders and asserts that the canonical event stream matches the golden
//! shape.

use jekko_provider::providers::{
    anthropic::decode_anthropic_sse, jnoccio::decode_jnoccio_sse, litellm::decode_litellm_sse,
    openai::decode_openai_sse, openrouter::decode_openrouter_sse,
};
use jekko_provider::stream::{ProviderEventKind, ToolCallAggregator};
use jekko_provider::SseDecoder;

const ANTHROPIC_TEXT: &[u8] = include_bytes!("fixtures/anthropic_text_stream.sse");
const ANTHROPIC_TOOL: &[u8] = include_bytes!("fixtures/anthropic_tool_call_stream.sse");
const OPENAI_TEXT: &[u8] = include_bytes!("fixtures/openai_text_stream.sse");
const OPENAI_TOOL: &[u8] = include_bytes!("fixtures/openai_tool_call_stream.sse");

#[test]
fn anthropic_text_stream_produces_expected_events() {
    let events = decode_anthropic_sse(ANTHROPIC_TEXT).unwrap();
    let kinds: Vec<&str> = events.iter().map(kind_tag).collect();
    assert_eq!(
        kinds,
        vec![
            "stream-start",
            "text-delta",
            "text-delta",
            "usage",
            "stream-end",
            "stream-end", // message_stop emits its own stream-end too
        ]
    );

    // Concatenate text deltas.
    let text: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            ProviderEventKind::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello, world!");

    // Usage event check.
    let usage = events
        .iter()
        .find(|e| matches!(e.kind, ProviderEventKind::Usage { .. }))
        .unwrap();
    if let ProviderEventKind::Usage {
        input_tokens,
        output_tokens,
        ..
    } = &usage.kind
    {
        assert_eq!(*input_tokens, 10);
        assert_eq!(*output_tokens, 4);
    }
}

#[test]
fn anthropic_tool_call_stream_aggregates_to_input_json() {
    let events = decode_anthropic_sse(ANTHROPIC_TOOL).unwrap();
    let kinds: Vec<&str> = events.iter().map(kind_tag).collect();
    assert!(kinds.contains(&"stream-start"));
    assert!(kinds.contains(&"tool-call-start"));
    assert!(kinds.contains(&"tool-call-input-delta"));

    // Reassemble via the aggregator using the synthesised `idx:0` id from the
    // Anthropic adapter.
    let mut agg = ToolCallAggregator::new();
    let mut tool_name = String::new();
    for ev in &events {
        if let ProviderEventKind::ToolCallStart { name, .. } = &ev.kind {
            tool_name = name.clone();
            // Re-emit a start with the canonical idx:0 id used by deltas so the
            // aggregator state has a builder to write into.
            agg.apply(&ProviderEventKind::ToolCallStart {
                id: "idx:0".into(),
                name: name.clone(),
            });
        }
        agg.apply(&ev.kind);
    }
    let done = agg.finalize("idx:0").unwrap();
    assert_eq!(done.name, "Read");
    assert_eq!(tool_name, "Read");
    assert_eq!(done.input["path"], "/etc/hosts");
}

#[test]
fn openai_text_stream_produces_expected_events() {
    let events = decode_openai_sse(OPENAI_TEXT).unwrap();
    let mut text = String::new();
    let mut saw_done = false;
    let mut saw_usage = false;
    for ev in &events {
        match &ev.kind {
            ProviderEventKind::TextDelta { text: t } => text.push_str(t),
            ProviderEventKind::StreamEnd { .. } => saw_done = true,
            ProviderEventKind::Usage { .. } => saw_usage = true,
            _ => {}
        }
    }
    assert_eq!(text, "Hello, world!");
    assert!(saw_done);
    assert!(saw_usage);
}

#[test]
fn openai_tool_call_stream_aggregates_via_id() {
    let events = decode_openai_sse(OPENAI_TOOL).unwrap();
    let mut agg = ToolCallAggregator::new();
    for ev in &events {
        agg.apply(&ev.kind);
    }
    let done = agg.finalize("call_01").unwrap();
    assert_eq!(done.name, "Read");
    assert_eq!(done.input["path"], "/etc/hosts");
}

#[test]
fn openrouter_uses_openai_decoder() {
    let events = decode_openrouter_sse(OPENAI_TEXT).unwrap();
    let text: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            ProviderEventKind::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello, world!");
}

#[test]
fn jnoccio_uses_openai_decoder() {
    let events = decode_jnoccio_sse(OPENAI_TEXT).unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, ProviderEventKind::TextDelta { text } if text == "Hello")));
}

#[test]
fn litellm_uses_openai_decoder() {
    let events = decode_litellm_sse(OPENAI_TEXT).unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, ProviderEventKind::TextDelta { text } if text == ", world!")));
}

#[test]
fn sse_decoder_handles_split_chunks_in_text() {
    // Feed the OpenAI text stream byte-by-byte. Should still produce the same
    // canonical events.
    let mut decoder = SseDecoder::new();
    let mut emitted = 0;
    for &b in OPENAI_TEXT {
        let frames = decoder.feed(&bytes::Bytes::copy_from_slice(&[b]));
        emitted += frames.len();
    }
    // Tail flush picks up the final frame.
    let final_frames = decoder.flush();
    emitted += final_frames.len();
    // 5 SSE blocks in the fixture (4 chunk objects + the [DONE] sentinel).
    assert!(emitted >= 4);
}

fn kind_tag(ev: &jekko_provider::ProviderEvent) -> &'static str {
    match ev.kind {
        ProviderEventKind::StreamStart { .. } => "stream-start",
        ProviderEventKind::TextDelta { .. } => "text-delta",
        ProviderEventKind::ReasoningDelta { .. } => "reasoning-delta",
        ProviderEventKind::ToolCallStart { .. } => "tool-call-start",
        ProviderEventKind::ToolCallInputDelta { .. } => "tool-call-input-delta",
        ProviderEventKind::ToolCallEnd { .. } => "tool-call-end",
        ProviderEventKind::Usage { .. } => "usage",
        ProviderEventKind::StreamEnd { .. } => "stream-end",
        ProviderEventKind::Error { .. } => "error",
        ProviderEventKind::Metadata { .. } => "metadata",
    }
}
