use std::path::Path;

use anyhow::{anyhow, Result};
use jekko_store::db::Db;
use serde_json::json;

use crate::daemon_store;
use crate::events::{EventKind, EventSink};
use crate::hero_judge_eval::{parse_substitute_lane_value, synthetic_lane_value};
use crate::model_client::{kind_label, ModelCallReceipt, ModelClient};
use crate::model_policy::ModelTaskKind;
use crate::reasoning_io::parse_structured_model_json;

#[derive(Clone, Copy)]
pub(crate) struct HeroJudgeCompletionContext<'a> {
    pub repo: &'a Path,
    pub run_id: &'a str,
    pub db: &'a Db,
    pub sink: &'a EventSink,
    pub model_client: &'a dyn ModelClient,
    pub require_parsed_live_json: bool,
}

pub(crate) async fn complete_hero_json(
    ctx: HeroJudgeCompletionContext<'_>,
    kind: ModelTaskKind,
    generation: usize,
    prompt: &str,
) -> Result<(ModelCallReceipt, serde_json::Value)> {
    for attempt in 1..=3 {
        ctx.sink.emit(
            EventKind::ModelAttempt,
            json!({"kind": kind_label(kind), "attempt": attempt}),
        )?;
        let receipt = ctx.model_client.complete(kind, prompt, ctx.repo).await?;
        daemon_store::persist_model_receipt(ctx.db, ctx.run_id, &receipt)?;
        let outcome = classify_hero_completion(
            kind,
            generation,
            attempt,
            &receipt,
            ctx.require_parsed_live_json,
        );
        ctx.sink.emit(
            EventKind::ModelAttemptOutcome,
            model_outcome_payload(&receipt, attempt, outcome.state_label()),
        )?;
        if matches!(outcome, HeroCompletionDecision::Parsed(_)) {
            ctx.sink.emit(
                EventKind::ModelOutcome,
                model_outcome_payload(&receipt, attempt, outcome.state_label()),
            )?;
        }
        if receipt.budget_used.is_some() || receipt.budget_remaining.is_some() {
            ctx.sink.emit(
                EventKind::LiveBudget,
                json!({
                    "used": receipt.budget_used.unwrap_or(0),
                    "remaining": receipt.budget_remaining.unwrap_or(0),
                }),
            )?;
        }
        match outcome {
            HeroCompletionDecision::Parsed(value) => return Ok((receipt, value)),
            HeroCompletionDecision::ProviderSyntheticResponse(value) => {
                return Ok((receipt, value))
            }
            HeroCompletionDecision::LiveParseSubstitution(value) => {
                return Ok((receipt, value));
            }
            HeroCompletionDecision::RetryableFailure(error) => {
                if attempt < 3 {
                    continue;
                }
                daemon_store::mark_daemon_run(
                    ctx.db,
                    ctx.run_id,
                    "blocked",
                    &receipt.kind,
                    Some(&error),
                )?;
                return Err(anyhow!("model call failed: {error}"));
            }
            HeroCompletionDecision::FinalBlock(error) => {
                daemon_store::mark_daemon_run(
                    ctx.db,
                    ctx.run_id,
                    "blocked",
                    &receipt.kind,
                    Some(&error),
                )?;
                return Err(anyhow!("model call failed: {error}"));
            }
        }
    }
    daemon_store::mark_daemon_run(
        ctx.db,
        ctx.run_id,
        "blocked",
        "hero_judge_model_json",
        Some("invalid model JSON"),
    )?;
    Err(anyhow!(
        "hero/judge model JSON parse failed: invalid model JSON"
    ))
}

fn retryable_model_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    !lower.contains("live call budget exhausted")
        && !lower.contains("no provider configured")
        && !lower.contains("missing credential")
        && !lower.contains("deterministic model receipt rejected")
}

fn timeout_model_error(error: &str) -> bool {
    error.to_ascii_lowercase().contains("timed out")
}

fn model_outcome_payload(
    receipt: &ModelCallReceipt,
    attempt: usize,
    state: &str,
) -> serde_json::Value {
    json!({
        "kind": receipt.kind,
        "provider": receipt.provider,
        "model": receipt.model,
        "success": receipt.success,
        "attempt": attempt,
        "state": state,
        "latency_ms": receipt.latency_ms,
        "response_bytes": receipt.response.as_ref().map(|response| response.len()),
        "credential_policy": receipt.credential_policy,
        "selected_credential_user_id": receipt.selected_credential_user_id,
        "credential_user_id": receipt.credential_user_id,
        "retry_count": receipt.retry_count,
        "budget_used": receipt.budget_used,
        "budget_remaining": receipt.budget_remaining,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeroCompletionState {
    Parsed,
    RetryableFailure,
    FinalBlock,
    FakeProviderSyntheticResponse,
    LiveParseSubstitution,
}

impl HeroCompletionState {
    fn as_str(self) -> &'static str {
        match self {
            HeroCompletionState::Parsed => "parsed",
            HeroCompletionState::RetryableFailure => "retryable_failure",
            HeroCompletionState::FinalBlock => "final_block",
            HeroCompletionState::FakeProviderSyntheticResponse => {
                "fake_provider_synthetic_response"
            }
            HeroCompletionState::LiveParseSubstitution => "live_parse_substitution",
        }
    }
}

enum HeroCompletionDecision {
    Parsed(serde_json::Value),
    ProviderSyntheticResponse(serde_json::Value),
    LiveParseSubstitution(serde_json::Value),
    RetryableFailure(String),
    FinalBlock(String),
}

impl HeroCompletionDecision {
    fn state_label(&self) -> &'static str {
        match self {
            HeroCompletionDecision::Parsed(_) => HeroCompletionState::Parsed.as_str(),
            HeroCompletionDecision::ProviderSyntheticResponse(_) => {
                HeroCompletionState::FakeProviderSyntheticResponse.as_str()
            }
            HeroCompletionDecision::LiveParseSubstitution(_) => {
                HeroCompletionState::LiveParseSubstitution.as_str()
            }
            HeroCompletionDecision::RetryableFailure(_) => {
                HeroCompletionState::RetryableFailure.as_str()
            }
            HeroCompletionDecision::FinalBlock(_) => HeroCompletionState::FinalBlock.as_str(),
        }
    }
}

fn classify_hero_completion(
    kind: ModelTaskKind,
    generation: usize,
    attempt: usize,
    receipt: &ModelCallReceipt,
    require_parsed_live_json: bool,
) -> HeroCompletionDecision {
    if !receipt.success {
        let error = receipt
            .error
            .clone()
            .unwrap_or_else(|| "unknown model failure".to_string());
        if require_parsed_live_json && timeout_model_error(&error) {
            return HeroCompletionDecision::FinalBlock(error);
        }
        if timeout_model_error(&error) {
            return HeroCompletionDecision::LiveParseSubstitution(parse_substitute_lane_value(
                kind, generation,
            ));
        }
        if retryable_model_error(&error) && attempt < 3 {
            return HeroCompletionDecision::RetryableFailure(error);
        }
        return HeroCompletionDecision::FinalBlock(error);
    }

    let Some(text) = receipt.response.as_deref() else {
        return if attempt < 3 {
            HeroCompletionDecision::RetryableFailure("missing model response".to_string())
        } else {
            HeroCompletionDecision::FinalBlock("missing model response".to_string())
        };
    };
    match parse_structured_model_json(text) {
        Ok(value) => HeroCompletionDecision::Parsed(value),
        Err(_) if receipt.provider == "fake" => HeroCompletionDecision::ProviderSyntheticResponse(
            synthetic_lane_value(kind, generation),
        ),
        Err(_) if require_parsed_live_json && attempt < 3 => {
            HeroCompletionDecision::RetryableFailure(
                "live model response was not parseable JSON".to_string(),
            )
        }
        Err(_) if require_parsed_live_json => HeroCompletionDecision::FinalBlock(
            "live model response was not parseable JSON".to_string(),
        ),
        Err(_) if text.trim().is_empty() && attempt < 3 => {
            HeroCompletionDecision::RetryableFailure("empty model response".to_string())
        }
        Err(_) if text.trim().is_empty() => {
            HeroCompletionDecision::FinalBlock("empty model response".to_string())
        }
        Err(_) => HeroCompletionDecision::LiveParseSubstitution(parse_substitute_lane_value(
            kind, generation,
        )),
    }
}
