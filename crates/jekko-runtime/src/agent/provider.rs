//! Provider / model / credential / adapter selection helpers.
//!
//! Split out of [`crate::agent`] so the static lookup tables for catalog
//! entries, base URLs, and provider adapters live in one place.

use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jekko_provider::adapter::ProviderCredential;
use jekko_provider::key_pool::KeyPool;
use jekko_provider::providers::jnoccio::JNOCCIO_DEFAULT_API_KEY;
use jekko_provider::providers::{
    AnthropicAdapter, JNoccioAdapter, JekkoAdapter, LiteLlmAdapter, OpenAiAdapter,
    OpenRouterAdapter,
};
use jekko_provider::routing::recommended_model_id;
use jekko_provider::setup::{
    catalog_entry, choose_active_provider, EnvValue, ModelKeySource, CATALOG,
};

use crate::error::{RuntimeError, RuntimeResult};
use crate::key_balancer::KeyBalancer;
use jekko_core::provider::{
    Model, ModelStatus, ProviderApiInfo, ProviderCapabilities, ProviderCost, ProviderId,
    ProviderInterleaved, ProviderLimit, ProviderModalities,
};
use tokio::time::sleep;

use super::types::AgentTurnRequest;

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
    let runtime_snapshot = supported_runtime_snapshot(&snapshot);
    let selection = choose_active_provider(&runtime_snapshot, developer_unlocked);
    match selection.active_provider_id {
        Some(id) => Ok(id),
        None => Err(RuntimeError::invalid(NO_PROVIDER_CONFIGURED_MSG)),
    }
}

static JNOCCIO_READY_GUARD: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn jnoccio_ready_guard() -> &'static tokio::sync::Mutex<()> {
    JNOCCIO_READY_GUARD.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn jnoccio_extra_port() -> Option<u16> {
    std::env::var("JNOCCIO_EXTRA_PORT")
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
}

pub(super) async fn ensure_jnoccio_ready(start: &Path) -> RuntimeResult<()> {
    let _guard = jnoccio_ready_guard().lock().await;
    let extra_port = jnoccio_extra_port();
    let initial = tokio::task::spawn_blocking(move || {
        jekko_jnoccio_boot::health::probe_health_combined(extra_port)
    })
    .await
    .map_err(|err| RuntimeError::other(format!("jnoccio health probe failed: {err}")))?;
    if initial.reachable {
        return Ok(());
    }

    if !jekko_jnoccio_boot::unlock::is_unlocked() {
        return Err(RuntimeError::other(
            "jnoccio-fusion is not unlocked on this machine",
        ));
    }

    let Some(fusion_root) = jekko_jnoccio_boot::unlock::find_jnoccio_fusion_root_from(start) else {
        return Err(RuntimeError::other(
            "jnoccio-fusion checkout not found for runtime auto-boot",
        ));
    };

    tokio::task::spawn_blocking(move || jekko_jnoccio_boot::spawn::ensure_and_spawn(&fusion_root))
        .await
        .map_err(|err| RuntimeError::other(format!("jnoccio spawn failed: {err}")))?
        .map_err(|err| RuntimeError::other(format!("jnoccio spawn failed: {err}")))?;

    for _ in 0..6 {
        sleep(Duration::from_millis(1350)).await;
        let extra_port = jnoccio_extra_port();
        let result = tokio::task::spawn_blocking(move || {
            jekko_jnoccio_boot::health::probe_health_combined(extra_port)
        })
        .await
        .map_err(|err| RuntimeError::other(format!("jnoccio health probe failed: {err}")))?;
        if result.reachable {
            return Ok(());
        }
    }

    Err(RuntimeError::other(
        "jnoccio-fusion did not become reachable after restart",
    ))
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

pub(super) fn select_base_url(provider_id: &str) -> Option<String> {
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

pub(super) fn build_model(provider_id: &str, model_id: &str) -> RuntimeResult<Model> {
    let (api_npm, api_url) = match provider_id {
        "anthropic" => ("@ai-sdk/anthropic", "https://api.anthropic.com"),
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

pub(super) fn provider_adapter(
    provider_id: &str,
) -> RuntimeResult<Arc<dyn jekko_provider::ProviderAdapter>> {
    match provider_id {
        "anthropic" => Ok(Arc::new(AnthropicAdapter::new())),
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

#[cfg(test)]
fn env_snapshot() -> BTreeMap<String, EnvValue> {
    env_snapshot_for(CredentialSourcePolicy::from_env())
}

fn env_snapshot_for(credential_policy: CredentialSourcePolicy) -> BTreeMap<String, EnvValue> {
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

fn supported_runtime_snapshot(values: &BTreeMap<String, EnvValue>) -> BTreeMap<String, EnvValue> {
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
        "anthropic" | "jekko" | "openai" | "openrouter" | "jnoccio" | "litellm" | "llmgateway"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        prev_home: Option<std::ffi::OsString>,
        prev_jekko_home: Option<std::ffi::OsString>,
        prev_dev: Option<std::ffi::OsString>,
        prev_policy: Option<std::ffi::OsString>,
        prev_openai: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn install(home: &std::path::Path, dev_key: Option<&str>) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev_home = std::env::var_os("HOME");
            let prev_jekko_home = std::env::var_os("JEKKO_HOME");
            let prev_dev = std::env::var_os("JNOCCIO_DEVELOPER_KEY");
            let prev_policy = std::env::var_os("JEKKO_KEY_SOURCE_POLICY");
            let prev_openai = std::env::var_os("OPENAI_API_KEY");
            std::env::set_var("HOME", home);
            std::env::remove_var("JEKKO_HOME");
            std::env::remove_var("JEKKO_KEY_SOURCE_POLICY");
            std::env::remove_var("OPENAI_API_KEY");
            match dev_key {
                Some(v) => std::env::set_var("JNOCCIO_DEVELOPER_KEY", v),
                None => std::env::remove_var("JNOCCIO_DEVELOPER_KEY"),
            }
            reset_balancer_for_tests();
            Self {
                prev_home,
                prev_jekko_home,
                prev_dev,
                prev_policy,
                prev_openai,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match &self.prev_jekko_home {
                Some(v) => std::env::set_var("JEKKO_HOME", v),
                None => std::env::remove_var("JEKKO_HOME"),
            }
            match &self.prev_dev {
                Some(v) => std::env::set_var("JNOCCIO_DEVELOPER_KEY", v),
                None => std::env::remove_var("JNOCCIO_DEVELOPER_KEY"),
            }
            match &self.prev_policy {
                Some(v) => std::env::set_var("JEKKO_KEY_SOURCE_POLICY", v),
                None => std::env::remove_var("JEKKO_KEY_SOURCE_POLICY"),
            }
            match &self.prev_openai {
                Some(v) => std::env::set_var("OPENAI_API_KEY", v),
                None => std::env::remove_var("OPENAI_API_KEY"),
            }
            reset_balancer_for_tests();
        }
    }

    #[test]
    fn provider_selection_skips_jnoccio_without_developer_unlock() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::install(home.path(), None);

        let snapshot = env_snapshot();
        let selection =
            choose_active_provider(&snapshot, jekko_jnoccio_boot::unlock::is_unlocked());
        assert_ne!(selection.active_provider_id.as_deref(), Some("jnoccio"));
    }

    #[test]
    fn provider_selection_accepts_developer_key_from_home_env_file() {
        let home = TempDir::new().unwrap();
        fs::write(
            home.path().join(".env.jnoccio"),
            "JNOCCIO_DEVELOPER_KEY=file-secret\n",
        )
        .unwrap();
        let _guard = EnvGuard::install(home.path(), None);

        let snapshot = env_snapshot();
        let selection =
            choose_active_provider(&snapshot, jekko_jnoccio_boot::unlock::is_unlocked());
        assert_eq!(selection.active_provider_id.as_deref(), Some("jnoccio"));
    }

    #[test]
    fn provider_selection_sees_default_user_key_pool() {
        let home = TempDir::new().unwrap();
        let user_dir = home.path().join(".jekko/users/user");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(user_dir.join("llm.env"), "OPENROUTER_API_KEY=key\n").unwrap();
        let _guard = EnvGuard::install(home.path(), None);

        let snapshot = env_snapshot();
        let selection =
            choose_active_provider(&snapshot, jekko_jnoccio_boot::unlock::is_unlocked());
        assert_eq!(selection.active_provider_id.as_deref(), Some("openrouter"));
    }

    #[test]
    fn provider_selection_sees_extra_user_key_pool_without_unlock() {
        let home = TempDir::new().unwrap();
        let user_dir = home.path().join(".jekko/users/user_1");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(user_dir.join("llm.env"), "OPENROUTER_API_KEY=key\n").unwrap();
        let _guard = EnvGuard::install(home.path(), None);

        let snapshot = env_snapshot();
        let selection = choose_active_provider(
            &supported_runtime_snapshot(&snapshot),
            jekko_jnoccio_boot::unlock::is_unlocked(),
        );
        assert_eq!(selection.active_provider_id.as_deref(), Some("openrouter"));
    }

    #[test]
    fn provider_selection_skips_configured_but_unsupported_provider() {
        let home = TempDir::new().unwrap();
        let user_dir = home.path().join(".jekko/users/user_1");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(
            user_dir.join("llm.env"),
            "GEMINI_API_KEY=google-key\nOPENROUTER_API_KEY=openrouter-key\n",
        )
        .unwrap();
        let _guard = EnvGuard::install(home.path(), None);

        let snapshot = env_snapshot();
        let selection = choose_active_provider(
            &supported_runtime_snapshot(&snapshot),
            jekko_jnoccio_boot::unlock::is_unlocked(),
        );
        assert_eq!(selection.active_provider_id.as_deref(), Some("openrouter"));
    }

    #[test]
    fn jnoccio_credential_uses_local_default_without_developer_key() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::install(home.path(), None);

        let selected = select_credential("jnoccio", "jnoccio/jnoccio-fusion")
            .unwrap()
            .expect("jnoccio local credential");
        assert!(selected.user_id.is_none());
        assert!(matches!(
            selected.credential,
            ProviderCredential::ApiKey { ref key } if key == JNOCCIO_DEFAULT_API_KEY
        ));
    }

    #[test]
    fn users_only_ignores_process_env_key() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::install(home.path(), None);
        std::env::set_var("OPENAI_API_KEY", "process-key");
        std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");

        let snapshot = env_snapshot();
        let selection = choose_active_provider(&supported_runtime_snapshot(&snapshot), true);
        assert_eq!(selection.active_provider_id, None);
        assert!(select_credential("openai", "gpt-5").unwrap().is_none());
    }

    #[test]
    fn users_only_ignores_home_env_jnoccio_file() {
        let home = TempDir::new().unwrap();
        fs::write(
            home.path().join(".env.jnoccio"),
            "JNOCCIO_DEVELOPER_KEY=file-secret\n",
        )
        .unwrap();
        let _guard = EnvGuard::install(home.path(), None);
        std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");

        let snapshot = env_snapshot();
        let selection = choose_active_provider(&supported_runtime_snapshot(&snapshot), true);
        assert_ne!(selection.active_provider_id.as_deref(), Some("jnoccio"));
    }

    #[test]
    fn users_only_rejects_jnoccio_local_default() {
        let home = TempDir::new().unwrap();
        let user_dir = home.path().join(".jekko/users/user");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(
            user_dir.join("llm.env"),
            format!("JNOCCIO_DEVELOPER_KEY={JNOCCIO_DEFAULT_API_KEY}\n"),
        )
        .unwrap();
        let _guard = EnvGuard::install(home.path(), None);
        std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");
        reset_balancer_for_tests();

        assert!(select_credential("jnoccio", "jnoccio-fusion")
            .unwrap()
            .is_none());
    }

    #[test]
    fn users_only_allows_jnoccio_when_key_is_in_user_llm_env() {
        let home = TempDir::new().unwrap();
        let user_dir = home.path().join(".jekko/users/user_1");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(
            user_dir.join("llm.env"),
            "JNOCCIO_DEVELOPER_KEY=user-secret\n",
        )
        .unwrap();
        let _guard = EnvGuard::install(home.path(), None);
        std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");
        reset_balancer_for_tests();

        let snapshot = env_snapshot();
        let selection = choose_active_provider(&supported_runtime_snapshot(&snapshot), true);
        assert_eq!(selection.active_provider_id.as_deref(), Some("jnoccio"));
        let selected = select_credential("jnoccio", "jnoccio-fusion")
            .unwrap()
            .expect("user llm.env credential");
        assert_eq!(selected.user_id.as_deref(), Some("user_1"));
    }

    #[test]
    fn selected_credential_in_users_only_always_has_user_id() {
        let home = TempDir::new().unwrap();
        let user_dir = home.path().join(".jekko/users/user");
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(user_dir.join("llm.env"), "OPENAI_API_KEY=user-key\n").unwrap();
        let _guard = EnvGuard::install(home.path(), None);
        std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");
        reset_balancer_for_tests();

        let selected = select_credential("openai", "gpt-5")
            .unwrap()
            .expect("user llm.env credential");
        assert_eq!(selected.user_id.as_deref(), Some("user"));
    }
}
