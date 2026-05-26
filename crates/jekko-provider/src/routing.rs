//! Model recommendation routing table.
//!
//! Ported from `packages/jekko/src/model-routing/recommendations.generated.ts`
//! and `recommendations.ts`. The list is **regenerated** from the upstream
//! generator in TypeScript; this Rust copy is kept in sync by hand for now.
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Static recommendation map: `provider_id -> model_id`.
///
/// Returns the canonical "best" model for each provider, used to pre-fill
/// model selection when a user hasn't picked one. Matches the entries in
/// `recommendations.generated.ts` 1:1.
pub fn recommended_models() -> &'static BTreeMap<&'static str, &'static str> {
    static MAP: OnceLock<BTreeMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = BTreeMap::new();
        m.insert("openai", "gpt-5.3-codex");
        m.insert("anthropic", "claude-sonnet-4-5");
        m.insert("google", "gemini-2.5-flash");
        m.insert("openrouter", "openrouter-gpt-oss-120b-free");
        m.insert("groq", "groq-qwen3-32b");
        m.insert("cerebras", "cerebras-qwen-3-235b-a22b-instruct-2507");
        m.insert("mistral", "mistral-devstral-latest");
        m.insert("github", "github-codestral-2501");
        m.insert("nvidia", "nvidia-deepseek-v4-pro");
        m.insert("fireworks", "fireworks-deepseek-v4-pro");
        m.insert("dashscope", "alibaba-qwen3-coder-plus");
        m.insert("sambanova", "sambanova-gpt-oss-120b");
        m.insert("huggingface", "huggingface-qwen3-coder-next");
        m.insert("zai", "zai-glm-47-flash");
        m.insert("inception", "inception-mercury-2");
        m.insert("ai-gateway", "vercel-claude-sonnet-46");
        m.insert("kilo", "kilo-ling-26-1t-free");
        m.insert("cloudflare", "cloudflare-gpt-oss-120b");
        m.insert("jekko", "big-pickle");
        m.insert("jnoccio", "jnoccio-router");
        m
    })
}

/// Convenience accessor for the recommendation map, mirroring the TypeScript
/// `RECOMMENDED_MODELS` constant.
#[allow(non_upper_case_globals)]
pub static RECOMMENDED_MODELS: &str = "use recommended_models() instead";

/// Returns the recommended model id for a given provider id, if any.
///
/// Mirror of `recommendedModelID` in `recommendations.ts`.
pub fn recommended_model_id(provider_id: &str) -> Option<&'static str> {
    recommended_models().get(provider_id).copied()
}

/// Returns true when `(provider_id, model_id)` matches the canonical
/// recommendation for that provider.
///
/// Mirror of `isKnownRecommendedModel` in `recommendations.ts`.
pub fn is_known_recommended_model(provider_id: &str, model_id: &str) -> bool {
    recommended_model_id(provider_id) == Some(model_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommends_known_providers() {
        assert_eq!(recommended_model_id("anthropic"), Some("claude-sonnet-4-5"));
        assert_eq!(recommended_model_id("openai"), Some("gpt-5.3-codex"));
        assert_eq!(recommended_model_id("jekko"), Some("big-pickle"));
        assert_eq!(recommended_model_id("jnoccio"), Some("jnoccio-router"));
        assert_eq!(recommended_model_id("nonexistent"), None);
    }

    #[test]
    fn known_recommended_match() {
        assert!(is_known_recommended_model("anthropic", "claude-sonnet-4-5"));
        assert!(!is_known_recommended_model("anthropic", "gpt-5"));
    }
}
