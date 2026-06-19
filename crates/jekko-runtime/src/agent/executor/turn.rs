//! Streaming tool-call turn loop for [`ProviderAgentExecutor`].
//!
//! Holds the [`AgentExecutor`] impl for [`ProviderAgentExecutor`] plus the
//! small private helpers it relies on: HTTP status extraction, output-token
//! bounding, jnoccio retry detection / metadata parsing, provenance header
//! assembly, and provider-id specific routing tweaks.

use std::collections::BTreeMap;

use async_trait::async_trait;
use futures_util::StreamExt;
use jekko_provider::adapter::ProviderRequest;
use jekko_provider::stream::ProviderEventKind;
use jekko_provider::transform::{
    max_output_tokens, message as transform_message, options as transform_options,
    provider_options as transform_provider_options, schema as transform_schema,
    temperature as transform_temperature, top_k as transform_top_k, top_p as transform_top_p,
    ModelMessage, OptionsInput,
};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::error::RuntimeResult;
use crate::tool::{default_registry, ToolOutput};

use super::super::oneshot::{
    build_assistant_tool_message, build_system_prompt, build_tool_result_message, execute_tool,
};
use super::super::provider::{
    build_model, ensure_jnoccio_ready, record_credential_failure, record_credential_success,
    select_base_url, select_credential, select_model_id, select_provider_id,
    CredentialSourcePolicy,
};
use super::super::types::{AgentTurnRequest, AgentTurnResult};
use super::mock::{mock_agent_turn_result, mock_llm_enabled};
use super::{AgentExecutor, ProviderAgentExecutor};

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
            Some(super::super::provider::SelectedCredential {
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
        let mut provider_options = transform_provider_options(
            &model,
            transform_options(OptionsInput {
                model: &model,
                session_id: &session_seed,
                provider_options: None,
            }),
        );
        // ZYAL `quality_band` opt-in: jekko-runner sets the env var
        // per stage from the active model_policy.<role>.quality_band.
        // Flow it through provider_options as a top-level key; the
        // OpenAI body builder copies it into the request body, and
        // fusion's RequestProfile reads it from `extra`.
        if let Ok(band) = std::env::var("JEKKO_RUN_QUALITY_BAND") {
            let trimmed = band.trim();
            if !trimmed.is_empty() {
                provider_options.insert("quality_band".into(), json!(trimmed));
            }
        }

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
                // Accumulates OpenAI-compatible streaming tool calls (Start + InputDelta
                // with no explicit end event) as (id, name, args_json) in arrival order.
                let mut tool_builders: Vec<(String, String, String)> = Vec::new();
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
                        ProviderEventKind::ToolCallStart { id, name } => {
                            tool_builders.push((id, name, String::new()));
                        }
                        ProviderEventKind::ToolCallInputDelta { id, delta } => {
                            if let Some(b) = tool_builders.iter_mut().rev().find(|b| b.0 == id) {
                                b.2.push_str(&delta);
                            }
                        }
                        ProviderEventKind::ToolCallEnd { id, name, input } => {
                            // Adapters that emit an explicit end (e.g. Anthropic) deliver
                            // the parsed input directly; drop any partial builder for it.
                            tool_builders.retain(|b| b.0 != id);
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
                        ProviderEventKind::StreamEnd { .. } => {
                            // OpenAI-compatible adapters (openai/jnoccio/litellm) stream tool
                            // calls as Start + InputDelta with NO explicit end event; finalize
                            // any in-flight builders here by parsing the accumulated argument
                            // JSON. Without this, a tool call (esp. when the model returns only
                            // a tool call with null content) is silently dropped.
                            for (id, name, args) in tool_builders.drain(..) {
                                let input =
                                    serde_json::from_str::<Value>(&args).unwrap_or(Value::Null);
                                tool_calls.push(json!({
                                    "id": id,
                                    "name": name,
                                    "input": input,
                                }));
                            }
                            break;
                        }
                        ProviderEventKind::Usage { .. }
                        | ProviderEventKind::StreamStart { .. }
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
                        match execute_tool(
                            tool.as_ref(),
                            tool_input,
                            &request,
                            &session_seed,
                            &tool_call,
                            self.permissions.clone(),
                            self.sessions.clone(),
                        )
                        .await
                        {
                            Ok(out) => out,
                            // A failed/denied tool must NOT abort the turn — feed the error
                            // back to the model so it can recover (e.g. permission denied).
                            Err(err) => ToolOutput::text(
                                format!("tool {tool_name}"),
                                format!("ERROR: {err}"),
                            ),
                        }
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

pub(super) fn runtime_provenance_headers(
    request: &AgentTurnRequest,
    credential_policy: &str,
    credential_user_id: Option<&str>,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("x-jekko-client".to_string(), "jekko-runtime".to_string());
    let run_id_header = match std::env::var("JEKKO_ZYAL_RUN_ID").ok() {
        Some(value) if !value.trim().is_empty() => value,
        _ => request.session_id.clone(),
    };
    headers.insert("x-jekko-run-id".to_string(), run_id_header);
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

pub(super) fn api_model_id_for<'a>(provider_id: &str, model_id: &'a str) -> &'a str {
    if provider_id == "jnoccio" && model_id == "jnoccio-router" {
        "jnoccio-fusion"
    } else {
        model_id
    }
}
