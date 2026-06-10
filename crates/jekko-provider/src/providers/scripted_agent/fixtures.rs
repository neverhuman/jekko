use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{ProviderError, ProviderResult};

const BASIC_SCENARIO: &str = include_str!("basic.json");
const TOOL_READ_SCENARIO: &str = include_str!("tool-read.json");
const FAILURE_SCENARIO: &str = include_str!("failure.json");
const PROVIDER_ID: &str = "scripted_agent";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScriptedScenario {
    pub(super) id: String,
    pub(super) title: String,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) stages: Vec<ScriptedStage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScriptedStage {
    pub(super) when: StageWhen,
    pub(super) frames: Vec<ScriptedFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum StageWhen {
    Initial,
    AfterToolResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum ScriptedFrame {
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

pub(super) fn scenarios() -> ProviderResult<&'static [ScriptedScenario]> {
    static SCENARIOS: OnceLock<Result<Vec<ScriptedScenario>, String>> = OnceLock::new();
    SCENARIOS
        .get_or_init(load_scenarios)
        .as_deref()
        .map_err(|err| ProviderError::Json(err.to_string()))
}

fn load_scenarios() -> Result<Vec<ScriptedScenario>, String> {
    let fixtures = [BASIC_SCENARIO, TOOL_READ_SCENARIO, FAILURE_SCENARIO];
    let mut scenarios = Vec::with_capacity(fixtures.len());
    let mut seen = BTreeSet::new();
    for fixture in fixtures {
        let scenario: ScriptedScenario =
            serde_json::from_str(fixture).map_err(|err| err.to_string())?;
        validate_scenario(&scenario)?;
        if !seen.insert(scenario.id.clone()) {
            return Err(format!(
                "duplicate scripted_agent scenario id `{}`",
                scenario.id
            ));
        }
        scenarios.push(scenario);
    }
    Ok(scenarios)
}

fn validate_scenario(scenario: &ScriptedScenario) -> Result<(), String> {
    if scenario.id.trim().is_empty() {
        return Err("scripted_agent scenario id must not be blank".into());
    }
    if scenario.provider != PROVIDER_ID {
        return Err(format!(
            "scripted_agent scenario `{}` has provider `{}`",
            scenario.id, scenario.provider
        ));
    }
    if scenario.title.trim().is_empty() {
        return Err(format!(
            "scripted_agent scenario `{}` must have a title",
            scenario.id
        ));
    }
    if scenario.model.trim().is_empty() {
        return Err(format!(
            "scripted_agent scenario `{}` must have a model",
            scenario.id
        ));
    }
    if scenario.tags.iter().any(|tag| tag.trim().is_empty()) {
        return Err(format!(
            "scripted_agent scenario `{}` has a blank tag",
            scenario.id
        ));
    }
    if scenario.stages.is_empty() {
        return Err(format!(
            "scripted_agent scenario `{}` must have at least one stage",
            scenario.id
        ));
    }
    if !scenario
        .stages
        .iter()
        .any(|stage| stage.when == StageWhen::Initial)
    {
        return Err(format!(
            "scripted_agent scenario `{}` must have an initial stage",
            scenario.id
        ));
    }
    for stage in &scenario.stages {
        if stage.frames.is_empty() {
            return Err(format!(
                "scripted_agent scenario `{}` has an empty stage",
                scenario.id
            ));
        }
    }
    Ok(())
}
