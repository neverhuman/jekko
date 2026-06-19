use serde::{Deserialize, Serialize};

/// Lane population counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeroJudgePopulation {
    /// Hero candidate lanes.
    #[serde(default = "default_hero_lanes")]
    pub hero_lanes: usize,
    /// Judge patch lanes.
    #[serde(default = "default_judge_lanes")]
    pub judge_lanes: usize,
    /// Verifier lanes.
    #[serde(default = "default_verifier_lanes")]
    pub verifier_lanes: usize,
    /// Literature synthesis lanes.
    #[serde(default = "default_literature_lanes")]
    pub literature_lanes: usize,
    /// Red-team lanes.
    #[serde(default = "default_red_team_lanes")]
    pub red_team_lanes: usize,
    /// Maximum concurrent model/search lanes.
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,
}

impl Default for HeroJudgePopulation {
    fn default() -> Self {
        Self {
            hero_lanes: default_hero_lanes(),
            judge_lanes: default_judge_lanes(),
            verifier_lanes: default_verifier_lanes(),
            literature_lanes: default_literature_lanes(),
            red_team_lanes: default_red_team_lanes(),
            max_parallel: default_max_parallel(),
        }
    }
}

/// Runtime budgets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeroJudgeBudgets {
    /// Maximum model calls.
    #[serde(default = "default_model_calls")]
    pub model_calls: usize,
    /// Maximum search queries.
    #[serde(default = "default_search_queries")]
    pub search_queries: usize,
    /// Maximum searched pages/hits.
    #[serde(default = "default_search_pages")]
    pub search_pages: usize,
}

impl Default for HeroJudgeBudgets {
    fn default() -> Self {
        Self {
            model_calls: default_model_calls(),
            search_queries: default_search_queries(),
            search_pages: default_search_pages(),
        }
    }
}

/// Research config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeroJudgeResearchConfig {
    /// Enable research receipts.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Use live agent-search providers when `--live` and `AGENT_SEARCH_LIVE=1`.
    #[serde(default)]
    pub live_when_available: bool,
    /// Missing provider policy.
    #[serde(default)]
    pub missing_provider: HeroJudgeMissingProviderPolicy,
    /// Explicit query list.
    #[serde(default)]
    pub queries: Vec<String>,
}

impl Default for HeroJudgeResearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            live_when_available: false,
            missing_provider: HeroJudgeMissingProviderPolicy::SkipWithReceipt,
            queries: Vec::new(),
        }
    }
}

/// Missing search provider policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeroJudgeMissingProviderPolicy {
    /// Write a skipped receipt and continue.
    #[default]
    SkipWithReceipt,
    /// Fail the run.
    Fail,
}

/// Local evidence input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeroJudgeEvidenceInput {
    /// Stable evidence id.
    pub id: String,
    /// Evidence role.
    pub role: String,
    /// Relative or absolute path. Simple `*` globs are supported.
    pub path: String,
    /// Maximum bytes.
    #[serde(default = "default_evidence_max_bytes")]
    pub max_bytes: usize,
}

/// Promotion policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroJudgePromotionPolicy {
    /// Minimum deterministic host score.
    #[serde(default = "default_promotion_min_score")]
    pub min_score: f64,
    /// Replay canaries before promotion.
    #[serde(default = "default_true")]
    pub canary_replay: bool,
    /// Reject leaked fixture constants and hidden canaries.
    #[serde(default = "default_true")]
    pub anti_leak: bool,
    /// Optional operator-supplied objective oracle command. When set, each
    /// generation runs `sh -c <command>` in the run repo and grounds the
    /// promotion score in its exit status (pass/fail) instead of relying on
    /// model self-reported scores. A failing oracle hard-rejects promotion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective_command: Option<String>,
}

impl Default for HeroJudgePromotionPolicy {
    fn default() -> Self {
        Self {
            min_score: default_promotion_min_score(),
            canary_replay: true,
            anti_leak: true,
            objective_command: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_hero_lanes() -> usize {
    6
}

fn default_judge_lanes() -> usize {
    4
}

fn default_verifier_lanes() -> usize {
    2
}

fn default_literature_lanes() -> usize {
    2
}

fn default_red_team_lanes() -> usize {
    2
}

fn default_max_parallel() -> usize {
    8
}

fn default_model_calls() -> usize {
    48
}

fn default_search_queries() -> usize {
    12
}

fn default_search_pages() -> usize {
    24
}

fn default_evidence_max_bytes() -> usize {
    64 * 1024
}

fn default_promotion_min_score() -> f64 {
    0.75
}
