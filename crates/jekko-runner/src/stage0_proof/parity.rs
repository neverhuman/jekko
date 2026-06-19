use crate::evidence::LoadedEvidence;
use crate::parity_lab::{ParityCase, ParityPerfBudget, ParityStep};
use crate::port::PortTargetRequest;

use super::helpers::{command_tokens, evidence_topics, slug};

pub(crate) fn generate_seed_cases(
    target: &PortTargetRequest,
    evidence: &[LoadedEvidence],
    model_value: &serde_json::Value,
) -> Vec<ParityCase> {
    let topics = evidence_topics(evidence, 6);
    if topics.is_empty() {
        return vec![generic_smoke_case()];
    }
    let commands = command_tokens(evidence);
    let target_kind = slug(&target.target);
    topics
        .iter()
        .enumerate()
        .map(|(idx, topic)| {
            let command = commands
                .get(idx % commands.len().max(1))
                .cloned()
                .unwrap_or_else(|| topic.to_ascii_uppercase());
            ParityCase {
                id: format!("{}.{}.seed", target_kind, slug(topic)),
                tags: vec![
                    "required".to_string(),
                    "approved".to_string(),
                    "generated".to_string(),
                    "seed".to_string(),
                ],
                target_kind: target_kind.clone(),
                steps: vec![ParityStep {
                    send: command,
                    expect: "OK".to_string(),
                }],
                perf: Some(ParityPerfBudget {
                    p95_ms_max_ratio: model_value
                        .get("p95_ms_max_ratio")
                        .and_then(serde_json::Value::as_f64)
                        .or(Some(1.25)),
                }),
            }
        })
        .collect()
}

fn generic_smoke_case() -> ParityCase {
    ParityCase {
        id: "port.capture.request".to_string(),
        tags: vec![
            "required".to_string(),
            "approved".to_string(),
            "smoke".to_string(),
        ],
        target_kind: "generic".to_string(),
        steps: vec![ParityStep {
            send: "PING".to_string(),
            expect: "PONG".to_string(),
        }],
        perf: Some(ParityPerfBudget {
            p95_ms_max_ratio: Some(1.25),
        }),
    }
}
