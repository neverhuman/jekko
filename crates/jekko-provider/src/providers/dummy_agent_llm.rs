//! Deterministic local provider for agent workflow tests.
//!
//! `dummy_agent_llm` is a no-network, no-token provider backed by strict JSON
//! scenario fixtures. It implements the same [`crate::ProviderAdapter`] stream
//! contract as the HTTP providers so runtime tests can exercise normal text,
//! tool-call, and failure flows without API keys.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use async_trait::async_trait;
use futures_util::stream;
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::adapter::{ProviderAdapter, ProviderRequest};
use crate::error::{ProviderError, ProviderResult};
use crate::stream::{ProviderCapabilities, ProviderEvent, ProviderEventKind};
use crate::ProviderStream;

const BASIC_SCENARIO: &str = include_str!("dummy_agent_llm/basic.json");
const TOOL_READ_SCENARIO: &str = include_str!("dummy_agent_llm/tool-read.json");
const FAILURE_SCENARIO: &str = include_str!("dummy_agent_llm/failure.json");
const DEFAULT_SCENARIO_ID: &str = "basic";
const PROVIDER_ID: &str = "dummy_agent_llm";

/// Local deterministic provider adapter for scripted agent simulations.
#[derive(Debug, Clone, Copy, Default)]
pub struct DummyAgentLlmAdapter;

impl DummyAgentLlmAdapter {
    /// Construct a new dummy adapter.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProviderAdapter for DummyAgentLlmAdapter {
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DummyScenario {
    id: String,
    title: String,
    #[serde(default)]
    tags: Vec<String>,
    provider: String,
    model: String,
    stages: Vec<DummyStage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DummyStage {
    when: StageWhen,
    frames: Vec<DummyFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StageWhen {
    Initial,
    AfterToolResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum DummyFrame {
    StreamStart {
        #[serde(default)]
        model: Option<String>,
    },
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        #[serde(default)]
        cache_read_tokens: u64,
        #[serde(default)]
        cache_write_tokens: u64,
    },
    Metadata {
        metadata: Value,
    },
    StreamEnd {
        #[serde(default)]
        stop_reason: Option<String>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
struct TemplateContext {
    last_user_text: String,
    first_path: String,
}

impl TemplateContext {
    fn from_request(req: &ProviderRequest) -> Self {
        let last_user_text = last_message_text(&req.messages, "user").unwrap_or_default();
        let first_path = first_absolute_path(&last_user_text).unwrap_or_else(|| "README.md".into());
        Self {
            last_user_text,
            first_path,
        }
    }

    fn expand_str(&self, input: &str) -> String {
        input
            .replace("{{last_user_text}}", &self.last_user_text)
            .replace("{{first_path}}", &self.first_path)
    }

    fn expand_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.expand_str(s)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.expand_value(v)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.expand_value(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

fn select_scenario(req: &ProviderRequest) -> ProviderResult<&'static DummyScenario> {
    let id = option_scenario_id(req)
        .or_else(|| known_scenario_id(&req.api_model_id))
        .or_else(|| {
            known_scenario_id(req.model.rsplit('/').next().unwrap_or(req.model.as_str()))
        })
        .unwrap_or(DEFAULT_SCENARIO_ID);
    scenarios()?
        .iter()
        .find(|scenario| scenario.id == id)
        .ok_or_else(|| {
            ProviderError::ProviderEvent(format!("unknown dummy_agent_llm scenario `{id}`"))
        })
}

fn option_scenario_id(req: &ProviderRequest) -> Option<&str> {
    req.options
        .get("dummy_agent_llm")
        .and_then(|value| value.get("scenario_id").or_else(|| value.get("scenario")))
        .and_then(Value::as_str)
        .or_else(|| {
            req.options
                .get("dummy_agent_llm_scenario_id")
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

fn select_stage<'a>(scenario: &'a DummyScenario, req: &ProviderRequest) -> &'a DummyStage {
    let wanted = if has_tool_result(&req.messages) {
        StageWhen::AfterToolResult
    } else {
        StageWhen::Initial
    };
    scenario
        .stages
        .iter()
        .find(|stage| stage.when == wanted)
        .or_else(|| scenario.stages.iter().find(|stage| stage.when == StageWhen::Initial))
        .expect("validated fixture must contain an initial stage")
}

fn push_frame(
    events: &mut Vec<ProviderResult<ProviderEvent>>,
    frame: &DummyFrame,
    scenario: &DummyScenario,
    context: &TemplateContext,
) -> ProviderResult<()> {
    match frame {
        DummyFrame::StreamStart { model } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::StreamStart {
                model: Some(
                    model
                        .as_deref()
                        .map(|m| context.expand_str(m))
                        .unwrap_or_else(|| format!("{}/{}", scenario.provider, scenario.model)),
                ),
            })));
        }
        DummyFrame::TextDelta { text } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::TextDelta {
                text: context.expand_str(text),
            })));
        }
        DummyFrame::ReasoningDelta { text } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::ReasoningDelta {
                text: context.expand_str(text),
            })));
        }
        DummyFrame::ToolCall { id, name, input } => {
            let input = context.expand_value(input);
            let input_json = serde_json::to_string(&input)?;
            events.push(Ok(ProviderEvent::new(ProviderEventKind::ToolCallStart {
                id: id.clone(),
                name: name.clone(),
            })));
            events.push(Ok(ProviderEvent::new(ProviderEventKind::ToolCallInputDelta {
                id: id.clone(),
                delta: input_json,
            })));
            events.push(Ok(ProviderEvent::new(ProviderEventKind::ToolCallEnd {
                id: id.clone(),
                name: name.clone(),
                input,
            })));
        }
        DummyFrame::Usage {
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
        DummyFrame::Metadata { metadata } => {
            let metadata = context.expand_value(metadata);
            events.push(Ok(ProviderEvent::new(ProviderEventKind::Metadata { metadata })));
        }
        DummyFrame::StreamEnd { stop_reason } => {
            events.push(Ok(ProviderEvent::new(ProviderEventKind::StreamEnd {
                stop_reason: stop_reason.clone(),
            })));
        }
        DummyFrame::Error { message } => {
            events.push(Err(ProviderError::ProviderEvent(context.expand_str(message))));
        }
    }
    Ok(())
}

fn scenarios() -> ProviderResult<&'static [DummyScenario]> {
    static SCENARIOS: OnceLock<Result<Vec<DummyScenario>, String>> = OnceLock::new();
    SCENARIOS
        .get_or_init(load_scenarios)
        .as_deref()
        .map_err(|err| ProviderError::Json(err.to_string()))
}

fn load_scenarios() -> Result<Vec<DummyScenario>, String> {
    let fixtures = [BASIC_SCENARIO, TOOL_READ_SCENARIO, FAILURE_SCENARIO];
    let mut scenarios = Vec::with_capacity(fixtures.len());
    let mut seen = BTreeSet::new();
    for fixture in fixtures {
        let scenario: DummyScenario =
            serde_json::from_str(fixture).map_err(|err| err.to_string())?;
        validate_scenario(&scenario)?;
        if !seen.insert(scenario.id.clone()) {
            return Err(format!("duplicate dummy_agent_llm scenario id `{}`", scenario.id));
        }
        scenarios.push(scenario);
    }
    Ok(scenarios)
}

fn validate_scenario(scenario: &DummyScenario) -> Result<(), String> {
    if scenario.id.trim().is_empty() {
        return Err("dummy_agent_llm scenario id must not be blank".into());
    }
    if scenario.provider != PROVIDER_ID {
        return Err(format!(
            "dummy_agent_llm scenario `{}` has provider `{}`",
            scenario.id, scenario.provider
        ));
    }
    if scenario.title.trim().is_empty() {
        return Err(format!(
            "dummy_agent_llm scenario `{}` must have a title",
            scenario.id
        ));
    }
    if scenario.model.trim().is_empty() {
        return Err(format!(
            "dummy_agent_llm scenario `{}` must have a model",
            scenario.id
        ));
    }
    if scenario.tags.iter().any(|tag| tag.trim().is_empty()) {
        return Err(format!(
            "dummy_agent_llm scenario `{}` has a blank tag",
            scenario.id
        ));
    }
    if scenario.stages.is_empty() {
        return Err(format!(
            "dummy_agent_llm scenario `{}` must have at least one stage",
            scenario.id
        ));
    }
    if !scenario
        .stages
        .iter()
        .any(|stage| stage.when == StageWhen::Initial)
    {
        return Err(format!(
            "dummy_agent_llm scenario `{}` must have an initial stage",
            scenario.id
        ));
    }
    for stage in &scenario.stages {
        if stage.frames.is_empty() {
            return Err(format!(
                "dummy_agent_llm scenario `{}` has an empty stage",
                scenario.id
            ));
        }
    }
    Ok(())
}

fn last_message_text(messages: &[Value], role: &str) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some(role))
        .and_then(|message| content_text(message.get("content")?))
}

fn content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                if let Some(s) = part
                    .get("text")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
                {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(s);
                }
            }
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

fn has_tool_result(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("tool")
            || message
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part.get("type").and_then(Value::as_str) == Some("tool-result")
                    })
                })
    })
}

fn first_absolute_path(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| {
            token.trim_matches(|c: char| {
                matches!(c, ',' | '.' | ':' | ';' | ')' | '(' | '"' | '\'')
            })
        })
        .find(|token| token.starts_with('/') && token.len() > 1)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use serde_json::{json, Map};

    fn request(model: &str) -> ProviderRequest {
        ProviderRequest {
            model: format!("dummy_agent_llm/{model}"),
            api_model_id: model.to_string(),
            session_id: "sess-1".into(),
            system: vec![],
            messages: vec![json!({ "role": "user", "content": "inspect /tmp/example.txt" })],
            tools: vec![],
            tool_choice: None,
            options: Map::new(),
            headers: Default::default(),
            max_output_tokens: 1024,
            temperature: None,
            top_p: None,
            top_k: None,
            credential: None,
            base_url: None,
        }
    }

    #[test]
    fn fixtures_are_strictly_valid() {
        let loaded = scenarios().unwrap();
        assert_eq!(loaded.len(), 3);
        assert!(loaded.iter().any(|scenario| scenario.id == "basic"));
        assert!(loaded.iter().any(|scenario| scenario.id == "tool-read"));
        assert!(loaded.iter().any(|scenario| scenario.id == "failure"));
    }

    #[tokio::test]
    async fn basic_scenario_is_deterministic() {
        let adapter = DummyAgentLlmAdapter::new();
        let mut first = adapter
            .stream(request("basic"), CancellationToken::new())
            .await
            .unwrap();
        let mut second = adapter
            .stream(request("basic"), CancellationToken::new())
            .await
            .unwrap();

        let mut first_text = String::new();
        while let Some(event) = first.next().await {
            if let ProviderEventKind::TextDelta { text } = event.unwrap().kind {
                first_text.push_str(&text);
            }
        }
        let mut second_text = String::new();
        while let Some(event) = second.next().await {
            if let ProviderEventKind::TextDelta { text } = event.unwrap().kind {
                second_text.push_str(&text);
            }
        }
        assert_eq!(first_text, second_text);
        assert!(first_text.contains("inspect /tmp/example.txt"));
    }

    #[tokio::test]
    async fn tool_scenario_expands_first_path() {
        let adapter = DummyAgentLlmAdapter::new();
        let mut stream = adapter
            .stream(request("tool-read"), CancellationToken::new())
            .await
            .unwrap();
        let mut input = None;
        while let Some(event) = stream.next().await {
            if let ProviderEventKind::ToolCallEnd { input: value, .. } = event.unwrap().kind {
                input = Some(value);
            }
        }
        assert_eq!(input.unwrap()["filePath"], json!("/tmp/example.txt"));
    }

    #[tokio::test]
    async fn failure_scenario_yields_provider_error() {
        let adapter = DummyAgentLlmAdapter::new();
        let mut stream = adapter
            .stream(request("failure"), CancellationToken::new())
            .await
            .unwrap();
        let mut saw_error = false;
        while let Some(event) = stream.next().await {
            if let Err(ProviderError::ProviderEvent(message)) = event {
                assert!(message.contains("scripted failure"));
                saw_error = true;
            }
        }
        assert!(saw_error);
    }
}
