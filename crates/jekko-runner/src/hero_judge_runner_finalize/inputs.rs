use std::path::{Path, PathBuf};

use jekko_store::db::Db;

use crate::events::EventSink;
use crate::hero_judge::{
    FrontierScore, HeroJudgeConfig, HeroJudgeLaneMetric, HeroJudgeQualityMetric,
    HeroJudgeReviewCard, HeroJudgeSearchReceipt, KnowledgeEntry, PromotionDecision, PromptVariant,
};

/// All the per-run state the finalizer needs.
pub(crate) struct FinalizeInputs<'a> {
    pub repo: &'a Path,
    pub run_id: &'a str,
    pub db: &'a Db,
    pub sink: &'a EventSink,
    pub config: &'a HeroJudgeConfig,
    pub source_runbook_sha256: String,
    pub objective: String,
    pub output_dir: PathBuf,
    pub generations: usize,
    pub lane_parallelism: usize,
    pub model_calls_used: usize,
    pub last_model_kind: Option<String>,
    pub last_decision: PromotionDecision,
    pub prompt_lineage: Vec<PromptVariant>,
    pub scoreboard: Vec<FrontierScore>,
    pub knowledge: Vec<KnowledgeEntry>,
    pub quality_metrics: Vec<HeroJudgeQualityMetric>,
    pub lane_metrics: Vec<HeroJudgeLaneMetric>,
    pub reviewer_cards: Vec<HeroJudgeReviewCard>,
    pub search_receipts: Vec<HeroJudgeSearchReceipt>,
}
