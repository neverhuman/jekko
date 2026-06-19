use std::path::Path;

use jekko_store::db::Db;

use crate::events::EventSink;
use crate::evidence::LoadedEvidence;
use crate::hero_judge::{
    FrontierScore, HeroJudgeConfig, HeroJudgeLaneMetric, HeroJudgeQualityMetric,
    HeroJudgeReviewCard, HeroJudgeSearchReceipt, KnowledgeEntry, PromotionDecision, PromptVariant,
};
use crate::model_client::ModelClient;

pub(in crate::hero_judge_runner_flow) struct GenerationInputs<'a> {
    pub repo: &'a Path,
    pub run_id: &'a str,
    pub db: &'a Db,
    pub sink: &'a EventSink,
    pub model_client: &'a dyn ModelClient,
    pub config: &'a HeroJudgeConfig,
    pub objective: &'a str,
    pub evidence: &'a [LoadedEvidence],
    pub search_receipts: &'a [HeroJudgeSearchReceipt],
    pub output_dir: &'a Path,
    pub generations: usize,
    pub lane_parallelism: usize,
    pub require_parsed_live_json: bool,
}

pub(in crate::hero_judge_runner_flow) struct GenerationState {
    pub model_calls_used: usize,
    pub last_model_kind: Option<String>,
    pub last_decision: PromotionDecision,
    pub prompt_lineage: Vec<PromptVariant>,
    pub scoreboard: Vec<FrontierScore>,
    pub knowledge: Vec<KnowledgeEntry>,
    pub quality_metrics: Vec<HeroJudgeQualityMetric>,
    pub lane_metrics: Vec<HeroJudgeLaneMetric>,
    pub reviewer_cards: Vec<HeroJudgeReviewCard>,
}
