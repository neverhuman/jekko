//! Provider / model / credential / adapter selection helpers.
//!
//! Split out of [`crate::agent`] so the static lookup tables for catalog
//! entries, base URLs, and provider adapters live in one place.

use std::env;
use std::sync::{Mutex, OnceLock};

use jekko_provider::adapter::ProviderCredential;
use jekko_provider::providers::jnoccio::JNOCCIO_DEFAULT_API_KEY;
use jekko_provider::routing::recommended_model_id;
use jekko_provider::setup::{catalog_entry, choose_active_provider};

use crate::error::{RuntimeError, RuntimeResult};
use crate::key_balancer::KeyBalancer;

use super::types::AgentTurnRequest;

mod env_snapshot;
mod jnoccio;
mod model;

pub(super) use env_snapshot::env_snapshot_for;
pub(super) use jnoccio::ensure_jnoccio_ready;
pub(super) use model::{build_model, provider_adapter, select_base_url};

// Canonical CredentialSourcePolicy lives in `zyal-core`. Re-exported here so
// existing `super::provider::CredentialSourcePolicy::from_env()` call sites in
// `executor.rs` and `oneshot.rs` keep compiling unchanged. Note: zyal-core's
// type-level `Default` is `UsersOnly` (the safer ZYAL-aligned default), but
// `from_env()` preserves the legacy jekko-runtime behavior where an unset env
// var still maps to `Any` (normal credential resolution).
pub(super) use zyal_core::CredentialSourcePolicy;

pub(super) fn select_provider_id(request: &AgentTurnRequest) -> RuntimeResult<String> {
    if let Some(provider) = request.provider.clone() {
        return Ok(provider);
    }
    let credential_policy = CredentialSourcePolicy::from_env();
    let snapshot = env_snapshot_for(credential_policy);
    let developer_unlocked =
        credential_policy.users_only() || jekko_jnoccio_boot::unlock::is_unlocked();
    let runtime_snapshot = env_snapshot::supported_runtime_snapshot(&snapshot);
    let selection = choose_active_provider(&runtime_snapshot, developer_unlocked);
    match selection.active_provider_id {
        Some(id) => Ok(id),
        None => Err(RuntimeError::invalid(NO_PROVIDER_CONFIGURED_MSG)),
    }
}

/// Error message used when no provider can be resolved from the env snapshot.
const NO_PROVIDER_CONFIGURED_MSG: &str =
    "no provider configured; set a provider explicitly or export provider credentials";

pub(super) fn select_model_id(
    provider_id: &str,
    request: &AgentTurnRequest,
) -> RuntimeResult<String> {
    if let Some(model) = request.model.clone() {
        return Ok(model);
    }
    if let Some(recommended) = recommended_model_id(provider_id) {
        return Ok(recommended.to_string());
    }
    match catalog_entry(provider_id).and_then(|entry| entry.recommended_model_id.map(String::from))
    {
        Some(id) => Ok(id),
        None => Err(RuntimeError::invalid(format!(
            "no recommended model known for `{provider_id}`"
        ))),
    }
}

/// Credential plus the user dir it came from. `user_id == None` means the
/// fallback `env::var()` path served the value — no per-user accounting is
/// available, so the balancer is not informed of the outcome.
#[derive(Debug, Clone)]
pub(super) struct SelectedCredential {
    pub credential: ProviderCredential,
    pub user_id: Option<String>,
}

pub(super) fn select_credential(
    provider_id: &str,
    model_id: &str,
) -> RuntimeResult<Option<SelectedCredential>> {
    let credential_policy = CredentialSourcePolicy::from_env();
    if let Some(pick) = balancer_pick(provider_id, model_id) {
        if credential_policy.users_only()
            && provider_id == "jnoccio"
            && matches!(
                pick.credential,
                ProviderCredential::ApiKey { ref key } if key == JNOCCIO_DEFAULT_API_KEY
            )
        {
            return Ok(None);
        }
        tracing::debug!(
            provider = provider_id,
            model = model_id,
            user = %pick.user_id,
            env_name = %pick.env_name,
            "selected credential from key pool",
        );
        return Ok(Some(SelectedCredential {
            credential: pick.credential,
            user_id: Some(pick.user_id),
        }));
    }
    if credential_policy.users_only() {
        return Ok(None);
    }
    let Some(entry) = catalog_entry(provider_id) else {
        return Ok(None);
    };
    for env_name in entry.env_names {
        if let Ok(value) = env::var(env_name) {
            if !value.trim().is_empty() {
                return Ok(Some(SelectedCredential {
                    credential: ProviderCredential::ApiKey { key: value },
                    user_id: None,
                }));
            }
        }
    }
    if provider_id == "jnoccio" {
        if let Some(value) = jekko_jnoccio_boot::unlock::developer_key() {
            return Ok(Some(SelectedCredential {
                credential: ProviderCredential::ApiKey { key: value },
                user_id: None,
            }));
        }
        return Ok(Some(SelectedCredential {
            credential: ProviderCredential::ApiKey {
                key: JNOCCIO_DEFAULT_API_KEY.to_string(),
            },
            user_id: None,
        }));
    }
    Ok(None)
}

/// Lazily-initialised process-wide balancer. Tests that need a clean slate
/// can call [`reset_balancer_for_tests`].
fn balancer() -> &'static Mutex<Option<KeyBalancer>> {
    static BALANCER: OnceLock<Mutex<Option<KeyBalancer>>> = OnceLock::new();
    BALANCER.get_or_init(|| Mutex::new(KeyBalancer::new(true)))
}

#[cfg(test)]
fn reset_balancer_for_tests() {
    if let Ok(mut guard) = balancer().lock() {
        *guard = KeyBalancer::new(true);
    }
}

fn balancer_pick(provider_id: &str, model_id: &str) -> Option<crate::key_balancer::KeyPick> {
    let mut guard = balancer().lock().ok()?;
    guard.as_mut()?.pick(provider_id, model_id)
}

/// Inform the balancer of a successful turn against `(provider, user, model)`.
pub(super) fn record_credential_success(provider_id: &str, user_id: &str, model_id: &str) {
    if let Ok(mut guard) = balancer().lock() {
        if let Some(bal) = guard.as_mut() {
            bal.record_success(provider_id, user_id, model_id);
        }
    }
}

/// Inform the balancer of a failed turn against `(provider, user, model)`.
/// Pass the upstream HTTP status when known; pass 0 for non-HTTP failures.
pub(super) fn record_credential_failure(
    provider_id: &str,
    user_id: &str,
    model_id: &str,
    http_status: u16,
) {
    if let Ok(mut guard) = balancer().lock() {
        if let Some(bal) = guard.as_mut() {
            if http_status >= 100 {
                bal.record_http(provider_id, user_id, model_id, http_status);
            } else {
                bal.record_failure(
                    provider_id,
                    user_id,
                    model_id,
                    crate::key_balancer::FailureKind::Other,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
