//! Model-key catalog and resolution.
//!
//! Ported from `packages/jekko/src/model-setup/model-keys.catalog.ts` and
//! `model-keys.ts`. Pure data and pure-functions only — actual env/file I/O
//! is performed by the runtime crate; this module just describes the
//! catalog and computes derived statuses given an env+file snapshot.
use serde::{Deserialize, Serialize};

/// One entry in the canonical model-key catalog.
///
/// Mirrors the TypeScript `CatalogEntry` shape from `model-keys.catalog.ts`.
/// Stored as `'static` borrowed slices to keep this catalog declared as a
/// `const`; this means [`CatalogEntry`] is not directly serde-deserialisable
/// but can be converted to an owned form via [`CatalogEntry::to_owned_entry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Provider identifier (e.g. `"openai"`).
    pub provider_id: &'static str,
    /// Env var names this provider reads, in priority order.
    pub env_names: &'static [&'static str],
    /// Optional sign-up URL printed by `jekko setup`.
    pub signup_url: Option<&'static str>,
    /// Recommended model id when this provider is the active one.
    pub recommended_model_id: Option<&'static str>,
    /// Selection priority (higher wins).
    pub priority: u32,
    /// Companion env vars that must also be configured for this provider to
    /// be considered fully configured (`CLOUDFLARE_ACCOUNT_ID` etc.).
    pub companion_env_names: Option<&'static [&'static str]>,
}

/// Where a key value was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelKeySource {
    /// `~/.jekko/jekko.env` file (legacy single-user layout).
    #[serde(rename = "jekko.env")]
    JekkoEnv,
    /// `~/.jekko/users/<user_id>/llm.env` file.
    /// The owning [`ModelKeyStatus`] / candidate carries the `user_id`.
    #[serde(rename = "users-llm.env")]
    UserLlmEnv,
    /// Process environment.
    #[serde(rename = "process-env")]
    ProcessEnv,
    /// Test-only injected content.
    #[serde(rename = "test-content")]
    TestContent,
}

/// Reasons a model-key candidate can be inactive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InactiveReason {
    /// No `JNOCCIO_DEVELOPER_KEY` present.
    NoDeveloperKey,
    /// Value is blank or missing.
    Blank,
    /// Provider is unsupported on this build.
    Unsupported,
    /// At least one of the companion env vars is missing.
    MissingCompanionEnv,
    /// Protected router is unavailable for this build.
    ProtectedRouterUnavailable,
}

/// Per-provider key status returned to the setup UI.
///
/// Mirrors `ModelKeyStatus` in `model-keys.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelKeyStatus {
    /// Env var name we resolved this status against.
    pub env_name: String,
    /// Provider identifier.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Whether at least one env name has a non-blank value.
    pub configured: bool,
    /// Whether this provider is selected as the active provider.
    pub active: bool,
    /// Where the value was sourced from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ModelKeySource>,
    /// User dir that produced the value when `source == UserLlmEnv`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "userID")]
    pub user_id: Option<String>,
    /// Sign-up URL (mirrored from the catalog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signup_url: Option<String>,
    /// Recommended model id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "recommendedModelID"
    )]
    pub recommended_model_id: Option<String>,
    /// Reason this candidate is inactive, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inactive_reason: Option<InactiveReason>,
    /// `"present"` or `"blank"` to mirror the TS redaction strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted: Option<String>,
}

/// The full catalog of model-key providers in priority order.
///
/// Sourced 1:1 from `model-keys.catalog.ts`.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        provider_id: "openai",
        env_names: &["OPENAI_API_KEY"],
        signup_url: Some("https://platform.openai.com/api-keys"),
        recommended_model_id: Some("gpt-5.3-codex"),
        priority: 90,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "anthropic",
        env_names: &["ANTHROPIC_API_KEY"],
        signup_url: Some("https://console.anthropic.com/settings/keys"),
        recommended_model_id: Some("claude-sonnet-4-5"),
        priority: 88,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "google",
        env_names: &[
            "GOOGLE_GENERATIVE_AI_API_KEY",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
        ],
        signup_url: Some("https://aistudio.google.com/apikey"),
        recommended_model_id: Some("gemini-2.5-flash"),
        priority: 86,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "openrouter",
        env_names: &["OPENROUTER_API_KEY"],
        signup_url: Some("https://openrouter.ai/keys"),
        recommended_model_id: Some("openrouter-gpt-oss-120b-free"),
        priority: 80,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "groq",
        env_names: &["GROQ_API_KEY"],
        signup_url: Some("https://console.groq.com/keys"),
        recommended_model_id: Some("groq-qwen3-32b"),
        priority: 78,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "cerebras",
        env_names: &["CEREBRAS_API_KEY"],
        signup_url: Some("https://cloud.cerebras.ai"),
        recommended_model_id: Some("cerebras-qwen-3-235b-a22b-instruct-2507"),
        priority: 77,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "mistral",
        env_names: &["MISTRAL_API_KEY"],
        signup_url: Some("https://console.mistral.ai/api-keys"),
        recommended_model_id: Some("mistral-devstral-latest"),
        priority: 76,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "github",
        env_names: &["GITHUB_TOKEN"],
        signup_url: Some("https://github.com/marketplace/models"),
        recommended_model_id: Some("github-codestral-2501"),
        priority: 75,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "nvidia",
        env_names: &["NVIDIA_API_KEY"],
        signup_url: Some("https://build.nvidia.com"),
        recommended_model_id: Some("nvidia-deepseek-v4-pro"),
        priority: 74,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "fireworks",
        env_names: &["FIREWORKS_API_KEY"],
        signup_url: Some("https://fireworks.ai/pricing"),
        recommended_model_id: Some("fireworks-deepseek-v4-pro"),
        priority: 73,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "dashscope",
        env_names: &["DASHSCOPE_API_KEY"],
        signup_url: Some("https://www.alibabacloud.com/help/en/model-studio/qwen-coder"),
        recommended_model_id: Some("alibaba-qwen3-coder-plus"),
        priority: 72,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "sambanova",
        env_names: &["SAMBANOVA_API_KEY"],
        signup_url: Some("https://cloud.sambanova.ai"),
        recommended_model_id: Some("sambanova-gpt-oss-120b"),
        priority: 71,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "huggingface",
        env_names: &["HF_TOKEN"],
        signup_url: Some("https://huggingface.co/settings/tokens"),
        recommended_model_id: Some("huggingface-qwen3-coder-next"),
        priority: 70,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "zai",
        env_names: &["ZAI_API_KEY"],
        signup_url: Some("https://z.ai/manage-apikey/apikey-list"),
        recommended_model_id: Some("zai-glm-47-flash"),
        priority: 69,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "inception",
        env_names: &["INCEPTION_API_KEY"],
        signup_url: Some("https://platform.inceptionlabs.ai"),
        recommended_model_id: Some("inception-mercury-2"),
        priority: 68,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "ai-gateway",
        env_names: &["AI_GATEWAY_API_KEY"],
        signup_url: Some("https://vercel.com/ai-gateway"),
        recommended_model_id: Some("vercel-claude-sonnet-46"),
        priority: 67,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "kilo",
        env_names: &["KILO_API_KEY"],
        signup_url: Some("https://app.kilo.ai"),
        recommended_model_id: Some("kilo-ling-26-1t-free"),
        priority: 66,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "cloudflare",
        env_names: &["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"],
        signup_url: Some("https://dash.cloudflare.com/profile/api-tokens"),
        recommended_model_id: Some("cloudflare-gpt-oss-120b"),
        priority: 65,
        companion_env_names: Some(&["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"]),
    },
    CatalogEntry {
        provider_id: "jekko",
        env_names: &["JEKKO_API_KEY"],
        signup_url: Some("https://jekko.ai/zen"),
        recommended_model_id: Some("big-pickle"),
        priority: 95,
        companion_env_names: None,
    },
    CatalogEntry {
        provider_id: "jnoccio",
        env_names: &["JNOCCIO_DEVELOPER_KEY"],
        signup_url: None,
        recommended_model_id: Some("jnoccio-router"),
        priority: 96,
        companion_env_names: None,
    },
];

/// Lookup helper: returns the catalog entry for `provider_id`, if any.
pub fn catalog_entry(provider_id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.provider_id == provider_id)
}

/// Parse a `KEY=VALUE` env file, skipping blank/comment lines. Used by every
/// surface that loads `~/.jekko/jekko.env` or `~/.jekko/users/<u>/llm.env`.
pub fn parse_env_lines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let mut split = trimmed.splitn(2, '=');
            let key = split.next()?.trim().to_string();
            let value = split.next()?.trim().to_string();
            Some((key, value))
        })
        .collect()
}

/// Fully-owned mirror of a [`CatalogEntry`] for JSON serialisation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedCatalogEntry {
    /// Provider id.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Env names.
    pub env_names: Vec<String>,
    /// Signup URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signup_url: Option<String>,
    /// Recommended model id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "recommendedModelID"
    )]
    pub recommended_model_id: Option<String>,
    /// Selection priority.
    pub priority: u32,
    /// Companion env names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion_env_names: Option<Vec<String>>,
}

impl CatalogEntry {
    /// Convert this borrowed catalog entry to an owned [`OwnedCatalogEntry`].
    pub fn to_owned_entry(&self) -> OwnedCatalogEntry {
        OwnedCatalogEntry {
            provider_id: self.provider_id.to_string(),
            env_names: self.env_names.iter().map(|s| (*s).to_string()).collect(),
            signup_url: self.signup_url.map(String::from),
            recommended_model_id: self.recommended_model_id.map(String::from),
            priority: self.priority,
            companion_env_names: self
                .companion_env_names
                .map(|c| c.iter().map(|s| (*s).to_string()).collect()),
        }
    }
}

/// Snapshot of one env var's resolved value and source.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvValue {
    /// The resolved value (`None` if blank/missing).
    pub value: Option<String>,
    /// Where this value came from.
    pub source: Option<ModelKeySource>,
}

/// Result returned by [`choose_active_provider`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderSelection {
    /// Active provider, if any.
    pub active_provider_id: Option<String>,
    /// `true` when `JNOCCIO_DEVELOPER_KEY` is set.
    pub developer_unlocked: bool,
}

/// Selects the active provider given a snapshot of all known env vars.
///
/// Mirrors the `chooseProvider` helper in `model-keys.ts`.
pub fn choose_active_provider(
    values: &std::collections::BTreeMap<String, EnvValue>,
    developer_unlocked: bool,
) -> ProviderSelection {
    let mut candidates = Vec::new();
    for entry in CATALOG {
        let present = entry.env_names.iter().find_map(|name| {
            values
                .get(*name)
                .and_then(|v| v.value.clone().filter(|s| !s.trim().is_empty()))
                .map(|v| (*name, v))
        });
        let Some((_env_name, _secret)) = present else {
            continue;
        };
        let configured_count = entry
            .env_names
            .iter()
            .filter(|n| {
                values
                    .get(**n)
                    .and_then(|v| v.value.as_ref())
                    .is_some_and(|s| !s.trim().is_empty())
            })
            .count();
        let missing_companion = entry
            .companion_env_names
            .map(|c| configured_count < c.len())
            .unwrap_or(false);
        let eligible = !missing_companion && (developer_unlocked || entry.provider_id != "jnoccio");
        if eligible {
            candidates.push(entry);
        }
    }
    if candidates.is_empty() {
        return ProviderSelection {
            active_provider_id: None,
            developer_unlocked,
        };
    }
    if developer_unlocked {
        if let Some(jnoccio) = candidates.iter().find(|c| c.provider_id == "jnoccio") {
            return ProviderSelection {
                active_provider_id: Some(jnoccio.provider_id.to_string()),
                developer_unlocked,
            };
        }
    }
    candidates.sort_by_key(|entry| std::cmp::Reverse(entry.priority));
    ProviderSelection {
        active_provider_id: candidates.first().map(|c| c.provider_id.to_string()),
        developer_unlocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env(values: &[(&str, &str)]) -> BTreeMap<String, EnvValue> {
        values
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_string(),
                    EnvValue {
                        value: Some((*v).to_string()),
                        source: Some(ModelKeySource::ProcessEnv),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn catalog_is_dense() {
        assert!(CATALOG.iter().any(|e| e.provider_id == "openai"));
        assert!(CATALOG.iter().any(|e| e.provider_id == "anthropic"));
        assert!(CATALOG.iter().any(|e| e.provider_id == "jnoccio"));
        let cf = catalog_entry("cloudflare").unwrap();
        assert_eq!(cf.companion_env_names.unwrap().len(), 2);
    }

    #[test]
    fn picks_highest_priority_when_multi_configured() {
        // OpenAI(90) + Anthropic(88) configured -> openai wins.
        let values = env(&[("OPENAI_API_KEY", "x"), ("ANTHROPIC_API_KEY", "y")]);
        let sel = choose_active_provider(&values, false);
        assert_eq!(sel.active_provider_id.as_deref(), Some("openai"));
    }

    #[test]
    fn jekko_outranks_anthropic_when_both_present() {
        // Jekko(95) beats Anthropic(88).
        let values = env(&[("JEKKO_API_KEY", "x"), ("ANTHROPIC_API_KEY", "y")]);
        let sel = choose_active_provider(&values, false);
        assert_eq!(sel.active_provider_id.as_deref(), Some("jekko"));
    }

    #[test]
    fn jnoccio_skipped_when_developer_locked() {
        // Without developer_unlocked, jnoccio is filtered out entirely.
        let values = env(&[("JNOCCIO_DEVELOPER_KEY", "secret")]);
        let sel = choose_active_provider(&values, false);
        assert!(sel.active_provider_id.is_none());
        let sel2 = choose_active_provider(&values, true);
        assert_eq!(sel2.active_provider_id.as_deref(), Some("jnoccio"));
    }

    #[test]
    fn cloudflare_requires_companion() {
        let values = env(&[("CLOUDFLARE_API_TOKEN", "x")]);
        let sel = choose_active_provider(&values, false);
        assert!(sel.active_provider_id.is_none());

        let values = env(&[
            ("CLOUDFLARE_API_TOKEN", "x"),
            ("CLOUDFLARE_ACCOUNT_ID", "y"),
        ]);
        let sel = choose_active_provider(&values, false);
        assert_eq!(sel.active_provider_id.as_deref(), Some("cloudflare"));
    }
}
