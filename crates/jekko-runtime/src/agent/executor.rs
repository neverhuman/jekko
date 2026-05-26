//! Provider-backed one-shot agent executor.
//!
//! Holds the [`AgentExecutor`] trait, the default [`ProviderAgentExecutor`],
//! and the streaming tool-call loop that talks to the provider adapter.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use jekko_provider::adapter::ProviderRequest;
use jekko_provider::stream::{ProviderEvent, ProviderEventKind};
use jekko_provider::transform::{
    max_output_tokens, message as transform_message, options as transform_options,
    provider_options as transform_provider_options, schema as transform_schema,
    temperature as transform_temperature, top_k as transform_top_k, top_p as transform_top_p,
    ModelMessage, OptionsInput,
};
use jekko_provider::ProviderStream;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::error::RuntimeResult;
use crate::tool::{default_registry, ToolOutput};

use super::oneshot::{
    build_assistant_tool_message, build_system_prompt, build_tool_result_message, execute_tool,
};
use super::provider::{
    build_model, ensure_jnoccio_ready, provider_adapter, record_credential_failure,
    record_credential_success, select_base_url, select_credential, select_model_id,
    select_provider_id, CredentialSourcePolicy,
};
use super::types::{AgentTurnRequest, AgentTurnResult};

/// Pluggable one-shot agent executor.
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    /// Execute a one-shot agent turn.
    async fn execute(&self, request: AgentTurnRequest) -> RuntimeResult<AgentTurnResult>;
}

/// Provider adapter lookup hook.
pub trait ProviderAdapterResolver: Send + Sync {
    /// Resolve a provider adapter for `provider_id`.
    fn resolve(&self, provider_id: &str)
        -> RuntimeResult<Arc<dyn jekko_provider::ProviderAdapter>>;
}

/// Default provider-backed executor.
pub struct ProviderAgentExecutor {
    permissions: Arc<crate::permission::PermissionService>,
    sessions: Arc<crate::session::SessionService>,
    resolver: Arc<dyn ProviderAdapterResolver>,
}

impl ProviderAgentExecutor {
    /// Construct a new provider-backed executor.
    pub fn new(
        permissions: Arc<crate::permission::PermissionService>,
        sessions: Arc<crate::session::SessionService>,
    ) -> Self {
        Self::with_resolver(
            permissions,
            sessions,
            Arc::new(DefaultProviderAdapterResolver),
        )
    }

    /// Construct with a caller-supplied adapter resolver.
    pub fn with_resolver(
        permissions: Arc<crate::permission::PermissionService>,
        sessions: Arc<crate::session::SessionService>,
        resolver: Arc<dyn ProviderAdapterResolver>,
    ) -> Self {
        Self {
            permissions,
            sessions,
            resolver,
        }
    }
}

#[derive(Debug, Default)]
struct DefaultProviderAdapterResolver;

impl ProviderAdapterResolver for DefaultProviderAdapterResolver {
    fn resolve(
        &self,
        provider_id: &str,
    ) -> RuntimeResult<Arc<dyn jekko_provider::ProviderAdapter>> {
        provider_adapter(provider_id)
    }
}

impl std::fmt::Debug for ProviderAgentExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderAgentExecutor")
            .field("permissions", &self.permissions)
            .field("sessions", &self.sessions)
            .field("resolver", &"<dyn ProviderAdapterResolver>")
            .finish()
    }
}

/// Environment variable that switches the provider executor into a
/// deterministic mock mode for PTY/integration tests.
///
/// When set to `"1"`, [`ProviderAgentExecutor::execute`] short-circuits
/// before adapter selection / credential lookup and returns a fixed
/// assistant response sourced from [`MOCK_RESPONSE_ENV`].
pub const MOCK_LLM_ENV: &str = "JEKKO_TUI_TEST_MOCK_LLM";

/// Environment variable holding the mock assistant payload.
///
/// The value may be either a plain string or a JSON object with a
/// `response` field (e.g. `{"response":"...","delayMs":25}`). When unset,
/// the executor falls back to a generic placeholder string.
pub const MOCK_RESPONSE_ENV: &str = "JEKKO_TUI_TEST_MOCK_RESPONSE";

/// Default mock assistant payload used when [`MOCK_RESPONSE_ENV`] is unset.
pub const MOCK_RESPONSE_DEFAULT: &str = "mocked assistant reply";

/// Extract the mock assistant text from [`MOCK_RESPONSE_ENV`].
///
/// Accepts either a plain string or a JSON object whose `response` field
/// holds the text. Defaults to [`MOCK_RESPONSE_DEFAULT`] when unset or
/// unparseable.
pub fn mock_assistant_text() -> String {
    let raw = std::env::var(MOCK_RESPONSE_ENV).unwrap_or_default();
    if raw.is_empty() {
        return MOCK_RESPONSE_DEFAULT.to_string();
    }
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&raw) {
        if let Some(text) = map.get("response").and_then(Value::as_str) {
            return text.to_string();
        }
    }
    raw
}

/// Build a deterministic [`ProviderStream`] from a fixed assistant text.
///
/// Emits `StreamStart` → `TextDelta` → `StreamEnd`, mirroring the shape
/// the real adapters yield. Used by the [`MOCK_LLM_ENV`] short-circuit
/// and by unit tests asserting the mock contract.
pub fn mock_assistant_stream() -> ProviderStream {
    let text = mock_assistant_text();
    let events = vec![
        Ok(ProviderEvent::new(ProviderEventKind::StreamStart {
            model: None,
        })),
        Ok(ProviderEvent::new(ProviderEventKind::TextDelta { text })),
        Ok(ProviderEvent::new(ProviderEventKind::StreamEnd {
            stop_reason: Some("stop".into()),
        })),
    ];
    Box::pin(stream::iter(events)) as ProviderStream
}

/// Build a deterministic [`AgentTurnResult`] from a fixed assistant text.
///
/// Used by [`MOCK_LLM_ENV`] to skip the entire provider/adapter stack
/// when the runtime is being exercised by PTY tests.
fn mock_agent_turn_result(request: &AgentTurnRequest) -> AgentTurnResult {
    let provider_id = request
        .provider
        .clone()
        .unwrap_or_else(|| "mock".to_string());
    let model_id = request
        .model
        .clone()
        .unwrap_or_else(|| "mock-model".to_string());
    AgentTurnResult {
        provider_id,
        model_id,
        assistant_text: mock_assistant_text(),
        reasoning_text: String::new(),
        tool_calls: Vec::new(),
        credential_source_policy: None,
        selected_credential_user_id: None,
        credential_user_id: None,
        router_metadata: None,
    }
}

/// Returns true when the runtime should short-circuit into the mock LLM.
pub fn mock_llm_enabled() -> bool {
    std::env::var(MOCK_LLM_ENV).as_deref() == Ok("1")
}

/// Extract the HTTP status from a [`jekko_provider::ProviderError`] when one
/// is present; falls back to `0` for non-HTTP errors.
fn http_status_of(err: &jekko_provider::ProviderError) -> u16 {
    match err {
        jekko_provider::ProviderError::Http { status, .. } => *status,
        _ => 0,
    }
}

fn bounded_max_output_tokens(model: &jekko_core::provider::Model) -> u32 {
    let default_limit = max_output_tokens(model);
    let Some(env_limit) = std::env::var("JEKKO_RUN_MAX_OUTPUT_TOKENS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
    else {
        return default_limit;
    };
    env_limit.min(default_limit)
}

fn jnoccio_metadata_user_id(metadata: &Value) -> Option<String> {
    metadata
        .get("credential_user_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn is_jnoccio_transport_failure(err: &jekko_provider::ProviderError) -> bool {
    matches!(err, jekko_provider::ProviderError::Transport(_))
}

#[async_trait]
impl AgentExecutor for ProviderAgentExecutor {
    async fn execute(&self, request: AgentTurnRequest) -> RuntimeResult<AgentTurnResult> {
        // JEKKO_TUI_TEST_MOCK_LLM=1 short-circuits the provider call with a
        // deterministic response so PTY tests can verify the chat-Enter →
        // render loop without needing real API keys or network. Honors
        // JEKKO_TUI_TEST_MOCK_RESPONSE (plain string or JSON `{response,...}`,
        // default "mocked assistant reply"). Sits above adapter selection +
        // credential lookup so it never reaches a real provider.
        if mock_llm_enabled() {
            return Ok(mock_agent_turn_result(&request));
        }
        let provider_id = select_provider_id(&request)?;
        let model_id = select_model_id(&provider_id, &request)?;
        let model = build_model(&provider_id, &model_id)?;
        let adapter = self.resolver.resolve(&provider_id)?;
        let selected = if let Some(credential) = request.credential.clone() {
            Some(super::provider::SelectedCredential {
                credential,
                user_id: request.selected_credential_user_id.clone(),
            })
        } else {
            select_credential(&provider_id, &model_id)?
        };
        let credential = selected.as_ref().map(|s| s.credential.clone());
        let selected_credential_user_id = selected.as_ref().and_then(|s| s.user_id.clone());
        let mut credential_user = selected_credential_user_id.clone();
        let credential_source_policy = CredentialSourcePolicy::from_env().as_str().to_string();
        let base_url = select_base_url(&provider_id);
        let tools_disabled = std::env::var("JEKKO_RUN_DISABLE_TOOLS").as_deref() == Ok("1");
        let registry = default_registry();
        let session_seed = if !request.ephemeral {
            request.session_id.clone()
        } else {
            format!("run_{}", uuid::Uuid::new_v4().simple())
        };
        let system = build_system_prompt(&request);
        let mut history: Vec<ModelMessage> = vec![
            json!({ "role": "system", "content": system }),
            json!({ "role": "user", "content": request.parsed_prompt.text }),
        ];
        let provider_options = transform_provider_options(
            &model,
            transform_options(OptionsInput {
                model: &model,
                session_id: &session_seed,
                provider_options: None,
            }),
        );

        let mut round = 0usize;
        let mut final_result = AgentTurnResult {
            provider_id: provider_id.clone(),
            model_id: model_id.clone(),
            assistant_text: String::new(),
            reasoning_text: String::new(),
            tool_calls: Vec::new(),
            credential_source_policy: Some(credential_source_policy.clone()),
            selected_credential_user_id: selected_credential_user_id.clone(),
            credential_user_id: credential_user.clone(),
            router_metadata: None,
        };
        let max_rounds = 2usize;
        'rounds: while round < max_rounds {
            round += 1;
            let mut retry_after_boot = false;
            'attempt: loop {
                if provider_id == "jnoccio" {
                    ensure_jnoccio_ready(&request.cwd).await?;
                }
                let tools = if tools_disabled {
                    Vec::new()
                } else {
                    registry
                        .catalog()
                        .into_iter()
                        .map(|tool| jekko_provider::adapter::ProviderTool {
                            name: tool.id,
                            description: Some(tool.description),
                            input_schema: transform_schema(&model, tool.schema),
                        })
                        .collect::<Vec<_>>()
                };
                let transformed_messages =
                    transform_message(history.clone(), &model, &provider_options);
                let headers = runtime_provenance_headers(
                    &request,
                    &credential_source_policy,
                    credential_user.as_deref(),
                );
                let provider_request = ProviderRequest {
                    model: format!("{provider_id}/{model_id}"),
                    api_model_id: api_model_id_for(&provider_id, &model_id).to_string(),
                    session_id: session_seed.clone(),
                    system: vec![],
                    messages: transformed_messages,
                    tools,
                    tool_choice: if tools_disabled {
                        None
                    } else {
                        Some("auto".into())
                    },
                    options: provider_options.clone(),
                    headers,
                    max_output_tokens: bounded_max_output_tokens(&model),
                    temperature: transform_temperature(&model),
                    top_p: transform_top_p(&model),
                    top_k: transform_top_k(&model),
                    credential: credential.clone(),
                    base_url: base_url.clone(),
                };

                let abort = CancellationToken::new();
                let mut stream = match adapter.stream(provider_request, abort).await {
                    Ok(s) => s,
                    Err(err) => {
                        if provider_id == "jnoccio"
                            && !retry_after_boot
                            && is_jnoccio_transport_failure(&err)
                        {
                            retry_after_boot = true;
                            ensure_jnoccio_ready(&request.cwd).await?;
                            continue 'attempt;
                        }
                        if should_record_outer_credential_failure(&provider_id) {
                            if let Some(user) = credential_user.as_deref() {
                                record_credential_failure(
                                    &provider_id,
                                    user,
                                    &model_id,
                                    http_status_of(&err),
                                );
                            }
                        }
                        return Err(err.into());
                    }
                };

                let mut assistant_text = String::new();
                let mut reasoning_text = String::new();
                let mut tool_calls = Vec::new();
                let mut router_metadata: Option<Value> = None;
                while let Some(item) = stream.next().await {
                    let event = match item {
                        Ok(ev) => ev,
                        Err(err) => {
                            if provider_id == "jnoccio"
                                && !retry_after_boot
                                && is_jnoccio_transport_failure(&err)
                            {
                                retry_after_boot = true;
                                ensure_jnoccio_ready(&request.cwd).await?;
                                continue 'attempt;
                            }
                            if should_record_outer_credential_failure(&provider_id) {
                                if let Some(user) = credential_user.as_deref() {
                                    record_credential_failure(
                                        &provider_id,
                                        user,
                                        &model_id,
                                        http_status_of(&err),
                                    );
                                }
                            }
                            return Err(err.into());
                        }
                    };
                    match event.kind {
                        ProviderEventKind::TextDelta { text } => assistant_text.push_str(&text),
                        ProviderEventKind::ReasoningDelta { text } => {
                            reasoning_text.push_str(&text)
                        }
                        ProviderEventKind::ToolCallEnd { id, name, input } => {
                            tool_calls.push(json!({
                                "id": id,
                                "name": name,
                                "input": input,
                            }));
                        }
                        ProviderEventKind::Metadata { metadata } => {
                            if provider_id == "jnoccio" {
                                if let Some(user_id) = jnoccio_metadata_user_id(&metadata) {
                                    credential_user = Some(user_id);
                                }
                                router_metadata = Some(metadata);
                            }
                        }
                        ProviderEventKind::StreamEnd { .. } => break,
                        ProviderEventKind::Usage { .. }
                        | ProviderEventKind::StreamStart { .. }
                        | ProviderEventKind::ToolCallStart { .. }
                        | ProviderEventKind::ToolCallInputDelta { .. }
                        | ProviderEventKind::Error { .. } => {}
                    }
                }
                if let Some(user) = credential_user.as_deref() {
                    record_credential_success(&provider_id, user, &model_id);
                }

                final_result = AgentTurnResult {
                    provider_id: provider_id.clone(),
                    model_id: model_id.clone(),
                    assistant_text: assistant_text.clone(),
                    reasoning_text: reasoning_text.clone(),
                    tool_calls: tool_calls.clone(),
                    credential_source_policy: Some(credential_source_policy.clone()),
                    selected_credential_user_id: selected_credential_user_id.clone(),
                    credential_user_id: credential_user.clone(),
                    router_metadata: router_metadata.clone(),
                };

                if tool_calls.is_empty() {
                    break 'rounds;
                }

                history.push(build_assistant_tool_message(
                    &assistant_text,
                    &reasoning_text,
                    &tool_calls,
                ));

                for tool_call in tool_calls {
                    let tool_name = tool_call
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("task");
                    let tool_input = tool_call.get("input").cloned().unwrap_or(Value::Null);
                    let tool = registry.get_case_insensitive(tool_name);
                    let output = if let Some(tool) = tool {
                        execute_tool(
                            tool.as_ref(),
                            tool_input,
                            &request,
                            &session_seed,
                            &tool_call,
                            self.permissions.clone(),
                            self.sessions.clone(),
                        )
                        .await?
                    } else {
                        ToolOutput::text(
                            format!("tool {tool_name}"),
                            format!("ERROR: unknown tool `{tool_name}`"),
                        )
                    };
                    history.push(build_tool_result_message(&tool_call, &output));
                }
                break 'attempt;
            }
        }

        Ok(final_result)
    }
}

fn runtime_provenance_headers(
    request: &AgentTurnRequest,
    credential_policy: &str,
    credential_user_id: Option<&str>,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("x-jekko-client".to_string(), "jekko-runtime".to_string());
    headers.insert(
        "x-jekko-run-id".to_string(),
        std::env::var("JEKKO_ZYAL_RUN_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| request.session_id.clone()),
    );
    headers.insert("x-jekko-session".to_string(), request.session_id.clone());
    headers.insert(
        "x-jekko-credential-policy".to_string(),
        credential_policy.to_string(),
    );
    if let Some(user_id) = credential_user_id.filter(|value| !value.trim().is_empty()) {
        headers.insert(
            "x-jekko-credential-user-id".to_string(),
            user_id.to_string(),
        );
    }
    for (env_name, header_name) in [
        ("JEKKO_ZYAL_RUN_ID", "x-jekko-zyal-run-id"),
        ("JEKKO_ZYAL_LANE_ID", "x-jekko-zyal-lane-id"),
        ("JEKKO_AGENT_ROLE", "x-jekko-agent-role"),
        ("JEKKO_PROCESS_ROLE", "x-jekko-process-role"),
    ] {
        if let Ok(value) = std::env::var(env_name) {
            if !value.trim().is_empty() {
                headers.insert(header_name.to_string(), value);
            }
        }
    }
    headers
}

fn should_record_outer_credential_failure(provider_id: &str) -> bool {
    provider_id != "jnoccio"
}

fn api_model_id_for<'a>(provider_id: &str, model_id: &'a str) -> &'a str {
    if provider_id == "jnoccio" && model_id == "jnoccio-router" {
        "jnoccio-fusion"
    } else {
        model_id
    }
}

#[cfg(test)]
mod mock_llm_hook_tests {
    //! Unit coverage for the [`MOCK_LLM_ENV`] short-circuit. Each test owns
    //! a tiny env-guard plus `#[serial]` because the cases share process
    //! env vars (`JEKKO_TUI_TEST_MOCK_*`) and would race in parallel.

    use super::*;
    use serial_test::serial;

    /// RAII guard that scopes a process-wide env var change to a single test.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[tokio::test]
    #[serial(jekko_mock_llm_env)]
    async fn mock_llm_hook_yields_text_delta() {
        let _mock = EnvVarGuard::set(MOCK_LLM_ENV, "1");
        let _resp = EnvVarGuard::set(MOCK_RESPONSE_ENV, "hello from mock");

        let mut stream = mock_assistant_stream();
        let mut events = Vec::new();
        while let Some(item) = stream.next().await {
            events.push(item.expect("mock stream never yields errors"));
        }
        assert_eq!(events.len(), 3, "mock stream is start + delta + end");
        assert!(
            matches!(events[0].kind, ProviderEventKind::StreamStart { .. }),
            "first event must be StreamStart: got {:?}",
            events[0].kind
        );
        match &events[1].kind {
            ProviderEventKind::TextDelta { text } => {
                assert_eq!(text, "hello from mock");
            }
            other => panic!("second event must be TextDelta: got {other:?}"),
        }
        assert!(
            matches!(events[2].kind, ProviderEventKind::StreamEnd { .. }),
            "third event must be StreamEnd: got {:?}",
            events[2].kind
        );
    }

    #[test]
    #[serial(jekko_mock_llm_env)]
    fn mock_assistant_text_accepts_plain_string() {
        let _resp = EnvVarGuard::set(MOCK_RESPONSE_ENV, "plain text reply");
        assert_eq!(mock_assistant_text(), "plain text reply");
    }

    #[test]
    #[serial(jekko_mock_llm_env)]
    fn mock_assistant_text_accepts_json_response_field() {
        let _resp = EnvVarGuard::set(
            MOCK_RESPONSE_ENV,
            r#"{"response":"json-shaped reply","delayMs":25}"#,
        );
        assert_eq!(mock_assistant_text(), "json-shaped reply");
    }

    #[test]
    #[serial(jekko_mock_llm_env)]
    fn mock_assistant_text_falls_back_when_unset() {
        // Ensure we restore whatever the surrounding env had.
        let original = std::env::var(MOCK_RESPONSE_ENV).ok();
        std::env::remove_var(MOCK_RESPONSE_ENV);
        let value = mock_assistant_text();
        match original {
            Some(v) => std::env::set_var(MOCK_RESPONSE_ENV, v),
            None => std::env::remove_var(MOCK_RESPONSE_ENV),
        }
        assert_eq!(value, MOCK_RESPONSE_DEFAULT);
    }

    #[test]
    #[serial(jekko_mock_llm_env)]
    fn mock_llm_enabled_requires_exact_one() {
        let _mock = EnvVarGuard::set(MOCK_LLM_ENV, "1");
        assert!(mock_llm_enabled());

        let _mock_zero = EnvVarGuard::set(MOCK_LLM_ENV, "0");
        assert!(!mock_llm_enabled());
    }

    #[test]
    #[serial(jekko_runtime_provenance_env)]
    fn runtime_provenance_headers_include_zyal_and_credential_fields() {
        let _run = EnvVarGuard::set("JEKKO_ZYAL_RUN_ID", "run-123");
        let _lane = EnvVarGuard::set("JEKKO_ZYAL_LANE_ID", "openqg");
        let _role = EnvVarGuard::set("JEKKO_AGENT_ROLE", "researcher");
        let request = AgentTurnRequest {
            prompt: "hello".to_string(),
            parsed_prompt: crate::prompt::parse("hello"),
            cwd: std::path::PathBuf::from("/tmp"),
            session_id: "session-1".to_string(),
            agent: None,
            provider: None,
            model: None,
            credential: None,
            selected_credential_user_id: None,
            ephemeral: true,
        };

        let headers = runtime_provenance_headers(&request, "users-only", Some("user_2"));

        assert_eq!(
            headers.get("x-jekko-client").map(String::as_str),
            Some("jekko-runtime")
        );
        assert_eq!(
            headers.get("x-jekko-run-id").map(String::as_str),
            Some("run-123")
        );
        assert_eq!(
            headers.get("x-jekko-zyal-lane-id").map(String::as_str),
            Some("openqg")
        );
        assert_eq!(
            headers.get("x-jekko-agent-role").map(String::as_str),
            Some("researcher")
        );
        assert_eq!(
            headers
                .get("x-jekko-credential-user-id")
                .map(String::as_str),
            Some("user_2")
        );
        assert_eq!(
            headers.get("x-jekko-credential-policy").map(String::as_str),
            Some("users-only")
        );
    }

    #[test]
    fn jnoccio_router_uses_gateway_model_id() {
        assert_eq!(
            api_model_id_for("jnoccio", "jnoccio-router"),
            "jnoccio-fusion"
        );
        assert_eq!(api_model_id_for("jnoccio", "custom"), "custom");
        assert_eq!(
            api_model_id_for("openrouter", "jnoccio-router"),
            "jnoccio-router"
        );
    }
}
