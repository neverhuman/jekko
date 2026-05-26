//! One-shot runtime entrypoint: prompt parsing, session bookkeeping, and the
//! glue helpers that translate tool / assistant turns into persisted rows.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::error::{RuntimeError, RuntimeResult};
use crate::processor::{assemble_system_prompt, SystemPromptParts};
use crate::prompt;
use crate::session::{AppendMessageInput, CreateSessionInput};
use crate::tool::{ToolContext, ToolOutput};
use crate::Runtime;
use jekko_core::project::ProjectId;

use super::provider::select_credential;
use super::types::{AgentTurnRequest, AgentTurnResult, RunRequest, RunResult};

/// Canonical agent name used when a request omits an explicit agent.
///
/// Centralised here so callers route through one named default instead of
/// duplicating the literal across the runtime. Changing this value affects
/// telemetry, session titles, and tool context routing.
const DEFAULT_AGENT: &str = "run";

impl Runtime {
    /// Prepare a one-shot agent run.
    ///
    /// The runtime now performs the actual provider-backed turn when a
    /// provider is configured. Session persistence remains optional via
    /// `ephemeral`.
    pub async fn run_oneshot(&self, request: RunRequest) -> RuntimeResult<RunResult> {
        if request.prompt.trim().is_empty() {
            return Err(RuntimeError::invalid("prompt is required"));
        }

        let parsed_prompt = prompt::parse(&request.prompt);
        let session_key = format!("run_{}", uuid::Uuid::new_v4().simple());
        let mut session = None;
        let mut user_message = None;

        if !request.ephemeral {
            let created = self
                .sessions
                .create(CreateSessionInput {
                    project_id: ProjectId::global().to_string(),
                    workspace_id: None,
                    parent_id: None,
                    directory: request.cwd.display().to_string(),
                    title: Some(build_title(&parsed_prompt, request.agent.as_deref())),
                })
                .await?;
            let message = self
                .sessions
                .append(AppendMessageInput {
                    session_id: created.id.clone(),
                    role: "user".to_string(),
                    data: serde_json::json!({
                        "text": parsed_prompt.text,
                        "agents": parsed_prompt.agents,
                        "files": parsed_prompt.files,
                        "agent": request.agent,
                        "provider": request.provider,
                        "model": request.model,
                        "ephemeral": request.ephemeral,
                    }),
                })
                .await?;
            session = Some(created);
            user_message = Some(message);
        }

        let prompt_text = request.prompt.clone();
        let agent = request.agent.clone();
        let provider = request.provider.clone();
        let model = request.model.clone();

        let provider_id = provider.clone();
        let model_id = model.clone();
        let selected_credential = match (provider_id.as_deref(), model_id.as_deref()) {
            (Some(provider_id), Some(model_id)) => select_credential(provider_id, model_id)?,
            _ => None,
        };
        let selected_credential_user_id = selected_credential
            .as_ref()
            .and_then(|selected| selected.user_id.clone());
        let credential = selected_credential
            .as_ref()
            .map(|selected| selected.credential.clone());
        let turn_result = self
            .agent_executor
            .execute(AgentTurnRequest {
                prompt: prompt_text,
                parsed_prompt: parsed_prompt.clone(),
                cwd: request.cwd.clone(),
                session_id: session
                    .as_ref()
                    .map(|s| s.id.to_string())
                    .unwrap_or(session_key),
                agent,
                provider: provider.clone(),
                model: model.clone(),
                credential,
                selected_credential_user_id: selected_credential_user_id.clone(),
                ephemeral: request.ephemeral,
            })
            .await;
        let turn = match turn_result {
            Ok(turn) => turn,
            Err(err) => {
                return Ok(RunResult {
                    parsed_prompt,
                    session,
                    message: user_message,
                    assistant_message: None,
                    assistant_text: None,
                    reasoning_text: None,
                    provider_id,
                    model_id,
                    tool_calls: Vec::new(),
                    credential_source_policy: Some(
                        super::provider::CredentialSourcePolicy::from_env()
                            .as_str()
                            .to_string(),
                    ),
                    selected_credential_user_id: selected_credential_user_id.clone(),
                    credential_user_id: selected_credential_user_id,
                    router_metadata: None,
                    accepted: true,
                    success: false,
                    error: Some(err.to_string()),
                });
            }
        };

        let assistant_message = if let Some(session_info) = &session {
            if turn.assistant_text.trim().is_empty() && turn.tool_calls.is_empty() {
                None
            } else {
                Some(
                    self.sessions
                        .append(AppendMessageInput {
                            session_id: session_info.id.clone(),
                            role: "assistant".to_string(),
                            data: assistant_payload(&turn),
                        })
                        .await?,
                )
            }
        } else {
            None
        };

        Ok(RunResult {
            parsed_prompt,
            session,
            message: user_message,
            assistant_message,
            assistant_text: Some(turn.assistant_text),
            reasoning_text: if turn.reasoning_text.is_empty() {
                None
            } else {
                Some(turn.reasoning_text)
            },
            provider_id: Some(turn.provider_id),
            model_id: Some(turn.model_id),
            tool_calls: turn.tool_calls,
            credential_source_policy: turn.credential_source_policy,
            selected_credential_user_id: turn.selected_credential_user_id,
            credential_user_id: turn.credential_user_id,
            router_metadata: turn.router_metadata,
            accepted: true,
            success: true,
            error: None,
        })
    }
}

fn assistant_payload(turn: &AgentTurnResult) -> Value {
    json!({
        "text": turn.assistant_text,
        "reasoning": turn.reasoning_text,
        "provider": turn.provider_id,
        "model": turn.model_id,
        "toolCalls": turn.tool_calls,
        "credentialSourcePolicy": turn.credential_source_policy,
        "selectedCredentialUserID": turn.selected_credential_user_id,
        "credentialUserID": turn.credential_user_id,
        "routerMetadata": turn.router_metadata,
    })
}

fn build_title(parsed: &prompt::ParsedPrompt, agent: Option<&str>) -> String {
    let prefix = agent.unwrap_or(DEFAULT_AGENT);
    let summary = parsed.text.trim();
    if summary.is_empty() {
        format!("Jekko {prefix}")
    } else {
        let first_line = summary.lines().next().unwrap_or(summary);
        let clipped: String = first_line.chars().take(48).collect();
        format!("Jekko {prefix}: {clipped}")
    }
}

pub(super) fn build_system_prompt(request: &AgentTurnRequest) -> String {
    let mut context = Vec::new();
    context.push(format!("Working directory: {}", request.cwd.display()));
    if let Some(agent) = request.agent.as_deref() {
        context.push(format!("Requested agent: {agent}"));
    }
    if !request.parsed_prompt.agents.is_empty() {
        context.push(format!(
            "Inline agent mentions: {}",
            request.parsed_prompt.agents.join(", ")
        ));
    }
    if !request.parsed_prompt.files.is_empty() {
        context.push(format!(
            "Inline file mentions: {}",
            request.parsed_prompt.files.join(", ")
        ));
    }
    assemble_system_prompt(&SystemPromptParts {
        instructions: "You are Jekko, a concise coding agent. Be factual, do not invent tool output, and if you cannot complete the request say so plainly.".into(),
        context,
        reminders: vec!["Prefer concrete next steps over speculation.".into()],
    })
}

pub(super) fn build_assistant_tool_message(
    assistant_text: &str,
    reasoning_text: &str,
    tool_calls: &[Value],
) -> Value {
    let mut content = Vec::new();
    if !assistant_text.is_empty() {
        content.push(json!({ "type": "text", "text": assistant_text }));
    }
    if !reasoning_text.is_empty() {
        content.push(json!({ "type": "reasoning", "text": reasoning_text }));
    }
    for call in tool_calls {
        content.push(json!({
            "type": "tool-call",
            "toolCallId": call.get("id").cloned().unwrap_or(Value::Null),
            "toolName": call.get("name").cloned().unwrap_or(Value::Null),
            "input": call.get("input").cloned().unwrap_or(Value::Null),
        }));
    }
    json!({ "role": "assistant", "content": content })
}

pub(super) fn build_tool_result_message(tool_call: &Value, output: &ToolOutput) -> Value {
    json!({
        "role": "tool",
        "content": [{
            "type": "tool-result",
            "toolCallId": tool_call.get("id").cloned().unwrap_or(Value::Null),
            "toolName": tool_call.get("name").cloned().unwrap_or(Value::Null),
            "input": tool_call.get("input").cloned().unwrap_or(Value::Null),
            "output": output.output.clone(),
            "metadata": output.metadata.clone(),
        }]
    })
}

pub(super) async fn execute_tool(
    tool: &dyn crate::tool::Tool,
    input: Value,
    request: &AgentTurnRequest,
    session_seed: &str,
    tool_call: &Value,
    permissions: Arc<crate::permission::PermissionService>,
    sessions: Arc<crate::session::SessionService>,
) -> RuntimeResult<ToolOutput> {
    let ctx = ToolContext {
        session_id: session_seed.to_string(),
        message_id: tool_call
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("tool-call")
            .to_string(),
        agent: request
            .agent
            .as_deref()
            .unwrap_or(DEFAULT_AGENT)
            .to_string(),
        cwd: request.cwd.clone(),
        permissions: Some(permissions),
        sessions: Some(sessions),
        extra: json!({}),
    };
    tool.execute(input, ctx).await
}
