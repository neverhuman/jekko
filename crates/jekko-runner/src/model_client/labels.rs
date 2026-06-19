use std::time::{SystemTime, UNIX_EPOCH};

use crate::model_policy::ModelTaskKind;

pub fn kind_label(kind: ModelTaskKind) -> &'static str {
    match kind {
        ModelTaskKind::Frame => "frame",
        ModelTaskKind::StageBrainstorm => "stage_brainstorm",
        ModelTaskKind::StageCritique => "stage_critique",
        ModelTaskKind::StageReduce => "stage_reduce",
        ModelTaskKind::PhaseBrainstorm => "phase_brainstorm",
        ModelTaskKind::Hypothesis => "hypothesis",
        ModelTaskKind::Critic => "critic",
        ModelTaskKind::Verifier => "verifier",
        ModelTaskKind::MemoryCurate => "memory_curate",
        ModelTaskKind::ParityGenerate => "parity_generate",
        ModelTaskKind::PerfClose => "perf_close",
        ModelTaskKind::HardEscalation => "hard_escalation",
        ModelTaskKind::Implement => "implement",
        ModelTaskKind::PhaseFinalize => "phase_finalize",
        ModelTaskKind::StuckDebug => "stuck_debug",
        ModelTaskKind::Healing => "healing",
        ModelTaskKind::PerfGap => "perf_gap",
        ModelTaskKind::Review => "review",
        ModelTaskKind::HeroGenerate => "hero_generate",
        ModelTaskKind::JudgePatch => "judge_patch",
        ModelTaskKind::LiteratureSynthesis => "literature_synthesis",
        ModelTaskKind::RedTeam => "red_team",
        ModelTaskKind::MetaJudge => "meta_judge",
        ModelTaskKind::KnowledgeCurate => "knowledge_curate",
    }
}

pub(super) fn receipt_id(prefix: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("model-{prefix}-{now}")
}
