//! Provider env-snapshot construction.
//!
//! Builds the `BTreeMap<String, EnvValue>` that
//! [`jekko_provider::setup::choose_active_provider`] consumes to decide which
//! provider to route a turn to. Also filters that snapshot down to the subset
//! of providers the runtime actually supports.

use std::collections::BTreeMap;
use std::env;

use jekko_provider::key_pool::KeyPool;
use jekko_provider::setup::{EnvValue, ModelKeySource, CATALOG};
use zyal_core::CredentialSourcePolicy;

#[cfg(test)]
pub(in crate::agent) fn env_snapshot() -> BTreeMap<String, EnvValue> {
    env_snapshot_for(CredentialSourcePolicy::from_env())
}

pub(in crate::agent) fn env_snapshot_for(
    credential_policy: CredentialSourcePolicy,
) -> BTreeMap<String, EnvValue> {
    let mut values = BTreeMap::new();
    if !credential_policy.users_only() {
        for entry in CATALOG {
            for env_name in entry.env_names {
                let value = env::var(env_name).ok().filter(|v| !v.trim().is_empty());
                values.insert(
                    (*env_name).to_string(),
                    EnvValue {
                        value,
                        source: Some(ModelKeySource::ProcessEnv),
                    },
                );
            }
            if let Some(companion) = entry.companion_env_names {
                for env_name in companion {
                    let value = env::var(env_name).ok().filter(|v| !v.trim().is_empty());
                    values.insert(
                        (*env_name).to_string(),
                        EnvValue {
                            value,
                            source: Some(ModelKeySource::ProcessEnv),
                        },
                    );
                }
            }
        }
    }
    merge_key_pool_snapshot(&mut values, true);
    if !credential_policy.users_only()
        && values
            .get("JNOCCIO_DEVELOPER_KEY")
            .and_then(|v| v.value.as_ref())
            .is_none()
    {
        if let Some(value) = jekko_jnoccio_boot::unlock::developer_key() {
            values.insert(
                "JNOCCIO_DEVELOPER_KEY".to_string(),
                EnvValue {
                    value: Some(value),
                    source: None,
                },
            );
        }
    }
    values
}

fn merge_key_pool_snapshot(values: &mut BTreeMap<String, EnvValue>, developer_unlocked: bool) {
    let Some(mut pool) = KeyPool::new(developer_unlocked) else {
        return;
    };
    for entry in CATALOG {
        if !is_supported_runtime_provider(entry.provider_id) {
            continue;
        }
        if pool.candidates(entry.provider_id).is_empty() {
            continue;
        }
        for env_name in entry.env_names {
            values
                .entry((*env_name).to_string())
                .and_modify(|value| {
                    if value.value.is_none() {
                        value.value = Some("present".to_string());
                        value.source = Some(ModelKeySource::UserLlmEnv);
                    }
                })
                .or_insert_with(|| EnvValue {
                    value: Some("present".to_string()),
                    source: Some(ModelKeySource::UserLlmEnv),
                });
        }
    }
}

pub(in crate::agent) fn supported_runtime_snapshot(
    values: &BTreeMap<String, EnvValue>,
) -> BTreeMap<String, EnvValue> {
    let mut filtered = BTreeMap::new();
    for entry in CATALOG {
        if !is_supported_runtime_provider(entry.provider_id) {
            continue;
        }
        for env_name in entry.env_names {
            if let Some(value) = values.get(*env_name) {
                filtered.insert((*env_name).to_string(), value.clone());
            }
        }
        if let Some(companion) = entry.companion_env_names {
            for env_name in companion {
                if let Some(value) = values.get(*env_name) {
                    filtered.insert((*env_name).to_string(), value.clone());
                }
            }
        }
    }
    filtered
}

fn is_supported_runtime_provider(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "anthropic"
            | "scripted_agent"
            | "jekko"
            | "openai"
            | "openrouter"
            | "jnoccio"
            | "litellm"
            | "llmgateway"
    )
}
