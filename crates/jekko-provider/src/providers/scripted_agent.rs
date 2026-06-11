//! Deterministic local provider for agent workflow tests.
//!
//! `scripted_agent` is a no-network, no-token provider backed by strict JSON
//! scenario fixtures. It implements the same [`crate::ProviderAdapter`] stream
//! contract as the HTTP providers so runtime tests can exercise normal text,
//! tool-call, and failure flows without API keys.

use async_trait::async_trait;
use futures_util::stream;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::adapter::{ProviderAdapter, ProviderRequest};
use crate::error::{ProviderError, ProviderResult};
use crate::stream::{ProviderCapabilities, ProviderEvent, ProviderEventKind};
use crate::ProviderStream;

#[path = "scripted_agent/context.rs"]
mod context;
#[path = "scripted_agent/fixtures.rs"]
mod fixtures;

use context::{has_tool_result, TemplateContext};
use fixtures::{scenarios, ScriptedFrame, ScriptedScenario, ScriptedStage, StageWhen};

const DEFAULT_SCENARIO_ID: &str = "basic";

/// Local deterministic provider adapter for scripted agent simulations.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScriptedAgentAdapter;

impl ScriptedAgentAdapter {
    /// Construct a new scripted adapter.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProviderAdapter for ScriptedAgentAdapter {
    async fn stream(
        &self,
        req: ProviderRequest,
        abort: CancellationToken,
    ) -> ProviderResult<ProviderStream> {
        if abort.is_cancelled() {
            return Err(ProviderError::Aborted);
        }

        let scenario = select_scenario(&req)?;
        let stage = select_stage(scenario, &req);
        let context = TemplateContext::from_request(&req);
        let mut events = Vec::new();
        for frame in &stage.frames {
            push_frame(&mut events, frame, scenario, &context)?;
        }
        Ok(Box::pin(stream::iter(events)) as ProviderStream)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            cache_control: false,
            tool_streaming: true,
        }
    }
}

fn select_scenario(req: &ProviderRequest) -> ProviderResult<&'static ScriptedScenario> {
    let id = option_scenario_id(req)
        .or_else(|| known_scenario_id(&req.api_model_id))
        .or_else(|| known_scenario_id(req.model.rsplit('/').next().unwrap_or(req.model.as_str())))
        .unwrap_or(DEFAULT_SCENARIO_ID);
    scenarios()?
        .iter()
        .find(|scenario| scenario.id == id)
        .ok_or_else(|| {
            ProviderError::ProviderEvent(format!("unknown scripted_agent scenario `{id}`"))
        })
}

fn option_scenario_id(req: &ProviderRequest) -> Option<&str> {
    req.options
        .get("scripted_agent")
        .and_then(|value| value.get("scenario_id").or_else(|| value.get("scenario")))
        .and_then(Value::as_str)
        .or_else(|| {
            req.options
                .get("scripted_agent_scenario_id")
                .and_then(Value::as_str)
        })
        .or_else(|| req.options.get("scenario_id").and_then(Value::as_str))
        .or_else(|| req.options.get("scenario").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn known_scenario_id(candidate: &str) -> Option<&'static str> {
    let candidate = candidate.trim();
    scenarios().ok()?.iter().find_map(|scenario| {
        if scenario.id == candidate {
            Some(scenario.id.as_str())
        } else {
            None
        }
    })
}

fn select_stage<'a>(scenario: &'a ScriptedScenario, req: &ProviderRequest) -> &'a ScriptedStage {
    let wanted = if has_tool_result(&req.messages) {
        StageWhen::AfterToolResult
    } else {
        StageWhen::Initial
    };
    scenario
        .stages
        .iter()
        .find(|stage| stage.when == wanted)
        .or_else(|| {
            scenario
                .stages
                .iter()
                .find(|stage| stage.when == StageWhen::Initial)
        })
        .expect("validated fixture must contain an initial stage")
}

fn push_frame(
    events: &mut Vec<ProviderResult<ProviderEvent>>,
    frame: &ScriptedFrame,
    scenario: &ScriptedScenario,
    context: &TemplateContext,
) -> ProviderResult<()> {
    match frame {
        ScriptedFrame::StreamStart { model } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::StreamStart {
                model: Some(
                    model
                        .as_deref()
                        .map(|m| context.expand_str(m))
                        .unwrap_or_else(|| format!("{}/{}", scenario.provider, scenario.model)),
                ),
            })));
        }
        ScriptedFrame::TextDelta { text } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::TextDelta {
                text: context.expand_str(text),
            })));
        }
        ScriptedFrame::ReasoningDelta { text } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::ReasoningDelta {
                text: context.expand_str(text),
            })));
        }
        ScriptedFrame::ToolCall { id, name, input } => {
            let input = context.expand_value(input);
            let input_json = serde_json::to_string(&input)?;
            events.push(Ok(ProviderEvent::new(ProviderEventKind::ToolCallStart {
                id: id.clone(),
                name: name.clone(),
            })));
            events.push(Ok(ProviderEvent::new(
                ProviderEventKind::ToolCallInputDelta {
                    id: id.clone(),
                    delta: input_json,
                },
            )));
            events.push(Ok(ProviderEvent::new(ProviderEventKind::ToolCallEnd {
                id: id.clone(),
                name: name.clone(),
                input,
            })));
        }
        ScriptedFrame::Usage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::Usage {
                input_tokens: *input_tokens,
                output_tokens: *output_tokens,
                cache_read_tokens: *cache_read_tokens,
                cache_write_tokens: *cache_write_tokens,
            })));
        }
        ScriptedFrame::Metadata { metadata } => {
            let metadata = context.expand_value(metadata);
            events.push(Ok(ProviderEvent::new(ProviderEventKind::Metadata {
                metadata,
            })));
        }
        ScriptedFrame::StreamEnd { stop_reason } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::StreamEnd {
                stop_reason: stop_reason.clone(),
            })));
        }
        ScriptedFrame::Error { message } => {
            events.push(Err(ProviderError::ProviderEvent(
                context.expand_str(message),
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "scripted_agent/tests.rs"]
mod tests;
