//! Provider options transform.
//!
//! Ported from `packages/jekko/src/provider/transform-options.ts`.
use jekko_core::provider::Model;
use serde_json::{json, Map, Value};

use super::shared::{sdk_key, OUTPUT_TOKEN_MAX};

/// Inputs for [`options`].
#[derive(Debug, Clone)]
pub struct OptionsInput<'a> {
    /// Target model.
    pub model: &'a Model,
    /// Session id (used as cache-key seed).
    pub session_id: &'a str,
    /// Optional provider options (passed in from the runtime).
    pub provider_options: Option<&'a Map<String, Value>>,
}

const fn slug_overrides(slug: &str) -> Option<&'static str> {
    match slug.as_bytes() {
        b"amazon" => Some("bedrock"),
        _ => None,
    }
}

/// Main per-call options computation.
///
/// Mirrors `options(...)` in `transform-options.ts`.
pub fn options(input: OptionsInput<'_>) -> Map<String, Value> {
    let mut result: Map<String, Value> = Map::new();
    let model = input.model;
    let session_id = input.session_id;
    let api_id = model.api.id.as_str();
    let api_id_lower = api_id.to_lowercase();
    let api_npm = model.api.npm.as_str();
    let provider_id = model.provider_id.as_str();

    if api_npm == "@ai-sdk/google-vertex/anthropic"
        || (!api_id.contains("claude") && api_npm == "@ai-sdk/anthropic")
    {
        result.insert("toolStreaming".into(), Value::Bool(false));
    }

    if provider_id == "openai" || api_npm == "@ai-sdk/openai" || api_npm == "@ai-sdk/github-copilot"
    {
        result.insert("store".into(), Value::Bool(false));
    }

    if api_npm == "@ai-sdk/azure" {
        result.insert("store".into(), Value::Bool(false));
        result.insert(
            "promptCacheKey".into(),
            Value::String(session_id.to_string()),
        );
    }

    if api_npm == "@openrouter/ai-sdk-provider" || api_npm == "@llmgateway/ai-sdk-provider" {
        result.insert("usage".into(), json!({ "include": true }));
        if api_id.contains("gemini-3") {
            result.insert("reasoning".into(), json!({ "effort": "high" }));
        }
    }

    if provider_id == "baseten"
        || (provider_id == "jekko" && matches!(api_id, "kimi-k2-thinking" | "glm-4.6"))
    {
        result.insert(
            "chat_template_args".into(),
            json!({ "enable_thinking": true }),
        );
    }

    if (provider_id.contains("zai") || provider_id.contains("zhipuai"))
        && api_npm == "@ai-sdk/openai-compatible"
    {
        result.insert(
            "thinking".into(),
            json!({ "type": "enabled", "clear_thinking": false }),
        );
    }

    let set_cache_key = input
        .provider_options
        .and_then(|o| o.get("setCacheKey"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if provider_id == "openai" || set_cache_key {
        result.insert(
            "promptCacheKey".into(),
            Value::String(session_id.to_string()),
        );
    }

    if (api_npm == "@ai-sdk/google" || api_npm == "@ai-sdk/google-vertex")
        && model.capabilities.reasoning
    {
        let mut thinking = json!({ "includeThoughts": true });
        if api_id.contains("gemini-3") {
            thinking["thinkingLevel"] = json!("high");
        }
        result.insert("thinkingConfig".into(), thinking);
    }

    if (api_npm == "@ai-sdk/anthropic" || api_npm == "@ai-sdk/google-vertex/anthropic")
        && (api_id_lower.contains("k2p")
            || api_id_lower.contains("kimi-k2.")
            || api_id_lower.contains("kimi-k2p"))
    {
        let budget = std::cmp::min(16_000_i64, (model.limit.output as i64) / 2 - 1);
        result.insert(
            "thinking".into(),
            json!({ "type": "enabled", "budgetTokens": budget }),
        );
    }

    if provider_id == "alibaba-cn"
        && model.capabilities.reasoning
        && api_npm == "@ai-sdk/openai-compatible"
        && !api_id_lower.contains("kimi-k2-thinking")
    {
        result.insert("enable_thinking".into(), Value::Bool(true));
    }

    if api_id.contains("gpt-5") && !api_id.contains("gpt-5-chat") {
        if !api_id.contains("gpt-5-pro") {
            result.insert("reasoningEffort".into(), Value::String("medium".into()));
            if api_npm == "@ai-sdk/openai"
                || api_npm == "@ai-sdk/azure"
                || api_npm == "@ai-sdk/github-copilot"
            {
                result.insert("reasoningSummary".into(), Value::String("auto".into()));
            }
        }

        if api_id.contains("gpt-5.")
            && !api_id.contains("codex")
            && !api_id.contains("-chat")
            && provider_id != "azure"
        {
            result.insert("textVerbosity".into(), Value::String("low".into()));
        }

        if provider_id.starts_with("jekko") {
            result.insert(
                "promptCacheKey".into(),
                Value::String(session_id.to_string()),
            );
            result.insert(
                "include".into(),
                Value::Array(vec![Value::String("reasoning.encrypted_content".into())]),
            );
            result.insert("reasoningSummary".into(), Value::String("auto".into()));
        }
    }

    if provider_id == "venice" {
        result.insert(
            "promptCacheKey".into(),
            Value::String(session_id.to_string()),
        );
    }

    if provider_id == "openrouter" {
        result.insert(
            "prompt_cache_key".into(),
            Value::String(session_id.to_string()),
        );
    }
    if api_npm == "@ai-sdk/gateway" {
        result.insert("gateway".into(), json!({ "caching": "auto" }));
    }

    result
}

/// Smaller options for the "small" model bias.
///
/// Mirrors `smallOptions(...)` in `transform-options.ts`.
pub fn small_options(model: &Model) -> Map<String, Value> {
    let mut out: Map<String, Value> = Map::new();
    let api_id = model.api.id.as_str();
    let api_npm = model.api.npm.as_str();
    let provider_id = model.provider_id.as_str();

    if provider_id == "openai" || api_npm == "@ai-sdk/openai" || api_npm == "@ai-sdk/github-copilot"
    {
        if api_id.contains("gpt-5") {
            if api_id.contains("5.") || api_id.contains("5-mini") {
                out.insert("store".into(), Value::Bool(false));
                out.insert("reasoningEffort".into(), Value::String("low".into()));
            } else {
                out.insert("store".into(), Value::Bool(false));
                out.insert("reasoningEffort".into(), Value::String("minimal".into()));
            }
            return out;
        }
        out.insert("store".into(), Value::Bool(false));
        return out;
    }
    if provider_id == "google" {
        if api_id.contains("gemini-3") {
            return json!({ "thinkingConfig": { "thinkingLevel": "minimal" } })
                .as_object()
                .cloned()
                .unwrap();
        }
        return json!({ "thinkingConfig": { "thinkingBudget": 0 } })
            .as_object()
            .cloned()
            .unwrap();
    }
    if provider_id == "openrouter" || provider_id == "llmgateway" {
        if api_id.contains("google") {
            return json!({ "reasoning": { "enabled": false } })
                .as_object()
                .cloned()
                .unwrap();
        }
        return json!({ "reasoningEffort": "minimal" })
            .as_object()
            .cloned()
            .unwrap();
    }
    if provider_id == "venice" {
        return json!({ "veniceParameters": { "disableThinking": true } })
            .as_object()
            .cloned()
            .unwrap();
    }
    out
}

/// Wrap raw options under the canonical SDK provider key.
///
/// Mirrors `providerOptions(...)` in `transform-options.ts`.
pub fn provider_options(model: &Model, opts: Map<String, Value>) -> Map<String, Value> {
    let api_id = model.api.id.as_str();
    let api_npm = model.api.npm.as_str();
    let provider_id = model.provider_id.as_str();

    if api_npm == "@ai-sdk/gateway" {
        let raw_slug = api_id.split_once('/').map(|(s, _)| s);
        let slug = raw_slug.map(|s| slug_overrides(s).unwrap_or(s));
        let gateway = opts.get("gateway").cloned();
        let rest: Map<String, Value> = opts.into_iter().filter(|(k, _)| k != "gateway").collect();
        let has = !rest.is_empty();

        let mut result: Map<String, Value> = Map::new();
        if let Some(g) = gateway.clone() {
            result.insert("gateway".into(), g);
        }
        if has {
            if let Some(slug) = slug {
                result.insert(slug.to_string(), Value::Object(rest));
            } else if let Some(Value::Object(g)) = gateway {
                let mut merged = g;
                for (k, v) in rest {
                    merged.insert(k, v);
                }
                result.insert("gateway".into(), Value::Object(merged));
            } else {
                result.insert("gateway".into(), Value::Object(rest));
            }
        }
        return result;
    }

    let uses_dot_split = matches!(
        api_npm,
        "@ai-sdk/openai-compatible" | "@ai-sdk/openai" | "@ai-sdk/anthropic"
    );
    let dotted: String = if uses_dot_split {
        provider_id
            .split('.')
            .next()
            .unwrap_or(provider_id)
            .to_string()
    } else {
        provider_id.to_string()
    };
    let key = sdk_key(api_npm).map(|s| s.to_string()).unwrap_or(dotted);

    if api_npm == "@ai-sdk/azure" {
        let mut result: Map<String, Value> = Map::new();
        result.insert("openai".into(), Value::Object(opts.clone()));
        result.insert("azure".into(), Value::Object(opts));
        return result;
    }
    let mut result: Map<String, Value> = Map::new();
    result.insert(key, Value::Object(opts));
    result
}

/// Cap a model's output limit at [`OUTPUT_TOKEN_MAX`].
pub fn max_output_tokens(model: &Model) -> u32 {
    let limit = model.limit.output as u32;
    let capped = std::cmp::min(limit, OUTPUT_TOKEN_MAX);
    if capped == 0 {
        OUTPUT_TOKEN_MAX
    } else {
        capped
    }
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
