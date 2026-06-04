//! Static provider lookup tables: model construction, adapter resolution, and
//! per-provider base-URL selection.

use std::env;
use std::sync::Arc;

use jekko_core::provider::{
    Model, ModelStatus, ProviderApiInfo, ProviderCapabilities, ProviderCost, ProviderId,
    ProviderInterleaved, ProviderLimit, ProviderModalities,
};
use jekko_provider::providers::{
    AnthropicAdapter, DummyAgentLlmAdapter, JNoccioAdapter, JekkoAdapter, LiteLlmAdapter,
    OpenAiAdapter, OpenRouterAdapter,
};

use crate::error::{RuntimeError, RuntimeResult};

pub(in crate::agent) fn select_base_url(provider_id: &str) -> Option<String> {
    if let Ok(value) = env::var("JEKKO_PROVIDER_BASE_URL") {
        if !value.trim().is_empty() {
            return Some(value);
        }
    }
    match provider_id {
        "litellm" | "llmgateway" => env::var("LITELLM_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty()),
        _ => None,
    }
}

pub(in crate::agent) fn build_model(provider_id: &str, model_id: &str) -> RuntimeResult<Model> {
    let (api_npm, api_url) = match provider_id {
        "anthropic" => ("@ai-sdk/anthropic", "https://api.anthropic.com"),
        "dummy_agent_llm" => ("dummy_agent_llm", "dummy://local"),
        "openai" => ("@ai-sdk/openai", "https://api.openai.com"),
        "openrouter" => ("@openrouter/ai-sdk-provider", "https://openrouter.ai/api"),
        "jekko" => ("@ai-sdk/openai-compatible", "https://api.jekko.ai"),
        "jnoccio" => ("@ai-sdk/openai", "http://127.0.0.1:4317"),
        "litellm" | "llmgateway" => ("@ai-sdk/openai-compatible", "http://127.0.0.1:4000"),
        other => {
            return Err(RuntimeError::invalid(format!(
                "unsupported provider `{other}`"
            )))
        }
    };
    Ok(Model {
        id: jekko_core::provider::ModelId::new(format!("{provider_id}/{model_id}")),
        provider_id: ProviderId::new(provider_id),
        api: ProviderApiInfo {
            id: model_id.to_string(),
            url: api_url.to_string(),
            npm: api_npm.to_string(),
        },
        name: model_id.to_string(),
        family: None,
        capabilities: ProviderCapabilities {
            temperature: true,
            reasoning: false,
            attachment: true,
            toolcall: true,
            input: ProviderModalities {
                text: true,
                audio: false,
                image: true,
                video: false,
                pdf: true,
            },
            output: ProviderModalities::default(),
            interleaved: ProviderInterleaved::Bool(false),
        },
        cost: ProviderCost {
            input: 0.0,
            output: 0.0,
            cache: Default::default(),
            experimental_over_200k: None,
        },
        limit: ProviderLimit {
            context: 200_000.0,
            input: None,
            output: 8192.0,
        },
        status: ModelStatus::Active,
        options: Default::default(),
        headers: Default::default(),
        release_date: "2025-01-01".into(),
        variants: None,
    })
}

pub(in crate::agent) fn provider_adapter(
    provider_id: &str,
) -> RuntimeResult<Arc<dyn jekko_provider::ProviderAdapter>> {
    match provider_id {
        "anthropic" => Ok(Arc::new(AnthropicAdapter::new())),
        "dummy_agent_llm" => Ok(Arc::new(DummyAgentLlmAdapter::new())),
        "jekko" => Ok(Arc::new(JekkoAdapter::new())),
        "openai" => Ok(Arc::new(OpenAiAdapter::new())),
        "openrouter" => Ok(Arc::new(OpenRouterAdapter::new())),
        "jnoccio" => Ok(Arc::new(JNoccioAdapter::new())),
        "litellm" | "llmgateway" => Ok(Arc::new(LiteLlmAdapter::new())),
        other => Err(RuntimeError::invalid(format!(
            "unsupported provider `{other}`"
        ))),
    }
}
