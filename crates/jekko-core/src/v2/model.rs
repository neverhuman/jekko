//! v2 model-catalog schema.
//!
//! Ported from `packages/jekko/src/v2/model.ts`.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::provider::{ModelId, ProviderId};
use crate::v2::schema::UtcMillis;

/// Variant identifier (`Schema.brand("VariantID")`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VariantId(pub String);

impl VariantId {
    /// Canonical default variant.
    pub const DEFAULT: &'static str = "default";

    /// Build the canonical `"default"` variant.
    pub fn default_variant() -> Self {
        Self(Self::DEFAULT.to_string())
    }

    /// Construct from any string-like value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// Family identifier (`Schema.brand("Family")`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Family(pub String);

/// Endpoint variant (`openai/responses`, `openai/completions`, `anthropic/messages`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Endpoint {
    /// OpenAI Responses endpoint.
    #[serde(rename = "openai/responses")]
    OpenAIResponses {
        /// API URL.
        url: String,
        /// Optional websocket-mode flag.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        websocket: Option<bool>,
    },
    /// OpenAI Completions endpoint.
    #[serde(rename = "openai/completions")]
    OpenAICompletions {
        /// API URL.
        url: String,
        /// Optional reasoning hint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<OpenAIReasoning>,
    },
    /// Anthropic Messages endpoint.
    #[serde(rename = "anthropic/messages")]
    AnthropicMessages {
        /// API URL.
        url: String,
    },
}

/// Reasoning hint flavour for the OpenAI Completions endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAIReasoning {
    /// `reasoning_content` field.
    ReasoningContent,
    /// `reasoning_details` field.
    ReasoningDetails,
}

/// Capability matrix (text-only, MIME-pattern-based).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Whether tool-use is supported.
    pub tools: bool,
    /// Input MIME patterns.
    pub input: Vec<String>,
    /// Output MIME patterns.
    pub output: Vec<String>,
}

/// Options block (HTTP headers + arbitrary body).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Options {
    /// HTTP headers.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Free-form body.
    #[serde(default)]
    pub body: BTreeMap<String, serde_json::Value>,
}

/// Cost pricing record (one per tier; the array on `Info::cost` can hold
/// several entries keyed by `Cost.tier`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// Tier descriptor (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<CostTier>,
    /// Cost per input unit.
    pub input: f64,
    /// Cost per output unit.
    pub output: f64,
    /// Cache cost.
    pub cache: CacheCost,
}

/// Tier descriptor for [`Cost`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CostTier {
    /// Context-window-keyed tier.
    Context {
        /// Tier size threshold.
        size: u32,
    },
}

/// Cache pricing.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct CacheCost {
    /// Cost per cache read.
    pub read: f64,
    /// Cost per cache write.
    pub write: f64,
}

/// Composite model reference (`provider/id/variant`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ref {
    /// Model id.
    pub id: ModelId,
    /// Provider id.
    #[serde(rename = "providerID")]
    pub provider_id: ProviderId,
    /// Variant id (often `"default"`).
    pub variant: VariantId,
}

/// Per-variant override (`Info.variants[i]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    /// Variant id.
    pub id: VariantId,
    /// HTTP headers.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Body overrides.
    #[serde(default)]
    pub body: BTreeMap<String, serde_json::Value>,
}

/// Model lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelLifecycleStatus {
    /// Alpha-quality.
    Alpha,
    /// Beta-quality.
    Beta,
    /// Inactive.
    Inactive,
    /// Generally available.
    Active,
    /// Provider key missing.
    Locked,
}

pub use ModelLifecycleStatus as Status;

/// Context/output limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limit {
    /// Context window in tokens.
    pub context: u32,
    /// Maximum input length (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u32>,
    /// Maximum output length in tokens.
    pub output: u32,
}

/// Model record (`Model.Info`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    /// Model id.
    pub id: ModelId,
    /// Provider id.
    #[serde(rename = "providerID")]
    pub provider_id: ProviderId,
    /// Family (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<Family>,
    /// Human-friendly name.
    pub name: String,
    /// Endpoint descriptor.
    pub endpoint: Endpoint,
    /// Capability matrix.
    pub capabilities: Capabilities,
    /// Default options.
    pub options: ModelOptions,
    /// Variant table.
    pub variants: Vec<Variant>,
    /// Time metadata.
    pub time: Time,
    /// Pricing tiers.
    pub cost: Vec<Cost>,
    /// Lifecycle status.
    pub status: Status,
    /// Token limits.
    pub limit: Limit,
}

/// Default options for a model, plus an optional variant hint.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelOptions {
    /// HTTP headers.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Body overrides.
    #[serde(default)]
    pub body: BTreeMap<String, serde_json::Value>,
    /// Optional default variant id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Time metadata (release date).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Time {
    /// Release date (UTC millis since epoch).
    pub released: UtcMillis,
}

/// Split a string like `"anthropic/claude-sonnet-4"` into `(provider, model)`.
///
/// Mirrors the `parse(input)` function in `packages/jekko/src/v2/model.ts`.
pub fn parse_ref(input: &str) -> (ProviderId, ModelId) {
    if let Some((provider, model)) = input.split_once('/') {
        (ProviderId::new(provider), ModelId::new(model))
    } else {
        (ProviderId::new(input), ModelId::new(String::new()))
    }
}
