use super::*;
use jekko_core::provider::{
    ModelId, ModelStatus, ProviderApiInfo, ProviderCacheCost, ProviderCapabilities, ProviderCost,
    ProviderId, ProviderInterleaved, ProviderLimit, ProviderModalities,
};
use std::collections::BTreeMap;

fn mock_model(provider_id: &str, npm: &str, api_id: &str) -> Model {
    Model {
        id: ModelId::new(format!("{provider_id}/test-model")),
        provider_id: ProviderId::new(provider_id),
        api: ProviderApiInfo {
            id: api_id.to_string(),
            url: "https://api.test.com".into(),
            npm: npm.to_string(),
        },
        name: "Test".into(),
        family: None,
        capabilities: ProviderCapabilities {
            temperature: true,
            reasoning: false,
            attachment: true,
            toolcall: true,
            input: ProviderModalities::default(),
            output: ProviderModalities::default(),
            interleaved: ProviderInterleaved::Bool(false),
        },
        cost: ProviderCost {
            input: 0.0,
            output: 0.0,
            cache: ProviderCacheCost::default(),
            experimental_over_200k: None,
        },
        limit: ProviderLimit {
            context: 200_000.0,
            input: None,
            output: 8192.0,
        },
        status: ModelStatus::Active,
        options: BTreeMap::new(),
        headers: BTreeMap::new(),
        release_date: "2025-01-01".into(),
        variants: None,
    }
}

#[test]
fn openai_sets_store_false() {
    let m = mock_model("openai", "@ai-sdk/openai", "gpt-4");
    let opts = options(OptionsInput {
        model: &m,
        session_id: "sess-1",
        provider_options: None,
    });
    assert_eq!(opts["store"], Value::Bool(false));
}

#[test]
fn set_cache_key_only_sets_prompt_cache_for_non_openai() {
    let m = mock_model(
        "anthropic",
        "@ai-sdk/anthropic",
        "claude-3-5-sonnet-20241022",
    );
    let mut prov_opts = Map::new();
    prov_opts.insert("setCacheKey".into(), Value::Bool(true));
    let opts = options(OptionsInput {
        model: &m,
        session_id: "sess-1",
        provider_options: Some(&prov_opts),
    });
    assert_eq!(opts["promptCacheKey"], "sess-1");
}

#[test]
fn azure_sets_store_and_prompt_cache_key() {
    let m = mock_model("azure", "@ai-sdk/azure", "gpt-4");
    let opts = options(OptionsInput {
        model: &m,
        session_id: "sess-1",
        provider_options: None,
    });
    assert_eq!(opts["store"], Value::Bool(false));
    assert_eq!(opts["promptCacheKey"], "sess-1");
}

#[test]
fn openai_provider_options_wraps_under_key() {
    let m = mock_model("openai", "@ai-sdk/openai", "gpt-4");
    let inner = options(OptionsInput {
        model: &m,
        session_id: "sess-1",
        provider_options: None,
    });
    let wrapped = provider_options(&m, inner.clone());
    assert!(wrapped.get("openai").is_some());
}

#[test]
fn azure_provider_options_wraps_under_both_keys() {
    let m = mock_model("azure", "@ai-sdk/azure", "gpt-4");
    let inner = options(OptionsInput {
        model: &m,
        session_id: "sess-1",
        provider_options: None,
    });
    let wrapped = provider_options(&m, inner.clone());
    assert!(wrapped.get("openai").is_some());
    assert!(wrapped.get("azure").is_some());
}

#[test]
fn gateway_provider_options_splits_slug() {
    // gateway with `anthropic/claude-...` api id should split-slug.
    let m = mock_model("vercel", "@ai-sdk/gateway", "anthropic/claude-sonnet-4");
    let mut input = Map::new();
    input.insert("foo".into(), Value::String("bar".into()));
    input.insert("gateway".into(), json!({ "caching": "auto" }));
    let wrapped = provider_options(&m, input);
    assert!(wrapped.get("gateway").is_some());
    assert!(wrapped.get("anthropic").is_some());
}

#[test]
fn max_output_tokens_caps() {
    let mut m = mock_model("anthropic", "@ai-sdk/anthropic", "x");
    m.limit.output = 64_000.0;
    assert_eq!(max_output_tokens(&m), 32_000);
    m.limit.output = 4096.0;
    assert_eq!(max_output_tokens(&m), 4096);
    m.limit.output = 0.0;
    assert_eq!(max_output_tokens(&m), 32_000);
}

#[test]
fn gpt5_textverbosity_low() {
    let m = mock_model("openai", "@ai-sdk/openai", "gpt-5.4");
    let opts = options(OptionsInput {
        model: &m,
        session_id: "sess-1",
        provider_options: None,
    });
    assert_eq!(opts["textVerbosity"], "low");
    assert_eq!(opts["reasoningEffort"], "medium");
}
