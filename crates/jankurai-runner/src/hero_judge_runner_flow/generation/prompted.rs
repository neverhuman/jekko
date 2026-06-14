use anyhow::Result;

use crate::hero_judge_eval::{build_evidence_keys, prompt_for};
use crate::hero_judge_runner_helpers::{run_lane_group, with_evolution_context};
use crate::model_policy::ModelTaskKind;

use super::types::GenerationInputs;

pub(super) async fn run_prompted_group(
    input: &GenerationInputs<'_>,
    generation: usize,
    role: &str,
    kind: ModelTaskKind,
    lanes: usize,
    evolution_context: &str,
) -> Result<Vec<crate::hero_judge::HeroJudgeLaneArtifact>> {
    let prompt = with_evolution_context(
        prompt_for(
            role,
            input.objective,
            generation,
            input.evidence,
            input.search_receipts,
        ),
        evolution_context,
    );
    // Resolvable evidence keys (loaded evidence + fetched receipts) so lane
    // grounding rewards citations that actually resolve, not just claims (U4).
    let known_keys = build_evidence_keys(input.evidence, input.search_receipts);
    run_lane_group(
        input.repo,
        input.run_id,
        input.db,
        input.sink,
        input.model_client,
        kind,
        generation,
        lanes,
        input.lane_parallelism,
        &prompt,
        input.require_parsed_live_json,
        &known_keys,
    )
    .await
}
