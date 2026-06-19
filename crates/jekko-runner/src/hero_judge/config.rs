use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model_policy::ModelPolicy;
use crate::superreasoning::SuperReasoningConfig;

use super::{
    HeroJudgeBudgets, HeroJudgeEvidenceInput, HeroJudgePopulation, HeroJudgePromotionPolicy,
    HeroJudgeResearchConfig,
};

/// Parsed top-level ZYAL runbook subset used by the Hero/Judge runner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeRunbook {
    /// Optional YAML id. The envelope id is accepted by the parser too.
    #[serde(default)]
    pub id: Option<String>,
    /// Optional job metadata.
    #[serde(default)]
    pub job: Option<HeroJudgeJob>,
    /// Hero/Judge runtime config.
    pub hero_judge: HeroJudgeConfig,
}

/// Minimal job metadata consumed for prompts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeJob {
    /// Job name.
    pub name: String,
    /// Objective text.
    pub objective: String,
}

/// Runtime configuration for dual hero/judge prompt evolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgeConfig {
    /// Runtime objective override.
    #[serde(default)]
    pub objective: Option<String>,
    /// Generation count requested by the runbook.
    #[serde(default = "default_generations")]
    pub generations: usize,
    /// Lane population counts.
    #[serde(default)]
    pub population: HeroJudgePopulation,
    /// Model/search budgets.
    #[serde(default)]
    pub budgets: HeroJudgeBudgets,
    /// Research behavior.
    #[serde(default)]
    pub research: HeroJudgeResearchConfig,
    /// Local evidence inputs.
    #[serde(default)]
    pub evidence: Vec<HeroJudgeEvidenceInput>,
    /// Promotion gate.
    #[serde(default)]
    pub promotion: HeroJudgePromotionPolicy,
    /// Superreasoning packet and gate policy.
    #[serde(default)]
    pub super_reasoning: SuperReasoningConfig,
    /// Provider-neutral model routing policy for Hero/Judge child calls.
    #[serde(default)]
    pub model_policy: ModelPolicy,
    /// Artifact output root relative to repo root.
    #[serde(default)]
    pub output_root: Option<String>,
}

impl HeroJudgeConfig {
    /// Generation count with a hard defensive cap.
    pub fn effective_generations(&self, override_max: Option<usize>) -> usize {
        let requested = override_max.unwrap_or(self.generations);
        requested.clamp(1, self.generations.max(1)).min(8)
    }

    /// Output root relative to the target repo.
    pub fn output_root(&self) -> PathBuf {
        self.output_root
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/openqg/hero-judge"))
    }
}

impl Default for HeroJudgeConfig {
    fn default() -> Self {
        Self {
            objective: None,
            generations: default_generations(),
            population: HeroJudgePopulation::default(),
            budgets: HeroJudgeBudgets::default(),
            research: HeroJudgeResearchConfig::default(),
            evidence: Vec::new(),
            promotion: HeroJudgePromotionPolicy::default(),
            super_reasoning: SuperReasoningConfig::default(),
            model_policy: ModelPolicy::default(),
            output_root: None,
        }
    }
}

fn default_generations() -> usize {
    2
}
