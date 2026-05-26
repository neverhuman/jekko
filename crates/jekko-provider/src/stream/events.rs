use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Canonical provider event kind emitted by the streaming layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProviderEventKind {
    /// Stream has started.
    StreamStart {
        /// Optional model id reported by the provider.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// New text delta available.
    TextDelta {
        /// Delta text.
        text: String,
    },
    /// Reasoning delta (e.g. extended thinking).
    ReasoningDelta {
        /// Delta text.
        text: String,
    },
    /// A tool call has started.
    ToolCallStart {
        /// Tool call id.
        id: String,
        /// Tool name.
        name: String,
    },
    /// Partial JSON delta for a tool call's input.
    ToolCallInputDelta {
        /// Tool call id this delta belongs to.
        id: String,
        /// Partial JSON delta as a string.
        delta: String,
    },
    /// A tool call has finished.
    ToolCallEnd {
        /// Tool call id.
        id: String,
        /// Tool name.
        name: String,
        /// Fully aggregated input JSON.
        input: serde_json::Value,
    },
    /// Usage report (token counts).
    Usage {
        /// Input tokens.
        input_tokens: u64,
        /// Output tokens.
        output_tokens: u64,
        /// Cache read tokens.
        #[serde(default)]
        cache_read_tokens: u64,
        /// Cache write tokens.
        #[serde(default)]
        cache_write_tokens: u64,
    },
    /// Stream has ended cleanly.
    StreamEnd {
        /// Provider-supplied stop reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
    },
    /// Out-of-band metadata emitted alongside the stream.
    Metadata {
        /// Arbitrary provider metadata payload.
        metadata: Value,
    },
    /// Provider returned an error mid-stream.
    Error {
        /// Error message.
        message: String,
    },
}

/// A canonical provider event with optional raw passthrough.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderEvent {
    /// Strongly-typed event kind.
    pub kind: ProviderEventKind,
    /// Original SSE event name (e.g. `"message_start"` for Anthropic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_event: Option<String>,
}

impl ProviderEvent {
    /// Construct from kind only (no raw event tag).
    pub fn new(kind: ProviderEventKind) -> Self {
        Self {
            kind,
            raw_event: None,
        }
    }

    /// Construct with a raw event name attached.
    pub fn with_raw(kind: ProviderEventKind, raw_event: impl Into<String>) -> Self {
        Self {
            kind,
            raw_event: Some(raw_event.into()),
        }
    }
}
