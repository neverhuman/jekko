//! Provider-neutral model-routing policy for ZYAL workflow tasks.

use serde::{Deserialize, Serialize};

/// Port workflow model task kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTaskKind {
    /// Request framing.
    Frame,
    /// Stage brainstorming.
    StageBrainstorm,
    /// Stage critique.
    StageCritique,
    /// Stage reduction/finalization.
    StageReduce,
    /// Phase brainstorming.
    PhaseBrainstorm,
    /// Hypothesis generation.
    Hypothesis,
    /// Critic pass.
    Critic,
    /// Executable/source verifier.
    Verifier,
    /// Durable memory curation.
    MemoryCurate,
    /// Parity case/report generation.
    ParityGenerate,
    /// Performance parity closure.
    PerfClose,
    /// Hard escalation.
    HardEscalation,
    /// Routine implementation.
    Implement,
    /// Phase finalization.
    PhaseFinalize,
    /// Stuck debugging.
    StuckDebug,
    /// Cross-phase healing.
    Healing,
    /// Performance gap analysis.
    PerfGap,
    /// Reviewer pass.
    Review,
    /// Hero candidate generation.
    HeroGenerate,
    /// Judge prompt patching.
    JudgePatch,
    /// Literature synthesis.
    LiteratureSynthesis,
    /// Adversarial red-team pass.
    RedTeam,
    /// Meta-judge reduction.
    MetaJudge,
    /// Verified knowledge curation.
    KnowledgeCurate,
}

/// One provider-neutral route. Both fields are optional: an empty route lets
/// the runtime choose the provider and model from its configured defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelRoute {
    /// Structured route form.
    Record(ModelRouteRecord),
    /// Compact `provider/model` or `model` string form.
    Compact(String),
}

impl Default for ModelRoute {
    fn default() -> Self {
        Self::Record(ModelRouteRecord::default())
    }
}

impl ModelRoute {
    /// Return this route as a normalized record.
    pub fn normalized(&self) -> ModelRouteRecord {
        match self {
            Self::Record(record) => record.clone(),
            Self::Compact(value) => ModelRouteRecord::from_compact(value),
        }
    }
}

/// Structured route record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRouteRecord {
    /// Optional provider id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Optional model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional model quality band (`any`/`top10`/`top20`/`top50`/`bottom20`).
    /// Forwarded to the fusion gateway via the OpenAI request's `extra` map
    /// so the router constrains selection by observed win-rate percentile.
    /// See `docs/ZYAL/MODEL_QUALITY_BAND.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_band: Option<String>,
}

impl ModelRouteRecord {
    fn from_compact(value: &str) -> Self {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Self::default();
        }
        match trimmed.split_once('/') {
            Some((provider, model)) if !provider.is_empty() && !model.is_empty() => Self {
                provider: Some(provider.to_string()),
                model: Some(model.to_string()),
                quality_band: None,
            },
            _ => Self {
                provider: None,
                model: Some(trimmed.to_string()),
                quality_band: None,
            },
        }
    }

    /// Whether neither provider nor model nor quality_band was explicitly
    /// configured. Used by `inherit_power_when_empty` so an empty role
    /// entry still falls through to the `power` policy.
    pub fn is_empty(&self) -> bool {
        self.provider.is_none() && self.model.is_none() && self.quality_band.is_none()
    }
}

/// Static route policy. Defaults are provider-neutral.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPolicy {
    /// Routine route.
    #[serde(default)]
    pub routine: ModelRoute,
    /// Power route for hard synthesis and review.
    #[serde(default)]
    pub power: ModelRoute,
    /// Critic route.
    #[serde(default)]
    pub critic: ModelRoute,
    /// Verifier route.
    #[serde(default)]
    pub verifier: ModelRoute,
    /// Meta-judge route.
    #[serde(default)]
    pub meta_judge: ModelRoute,
    /// Reproducer route.
    #[serde(default)]
    pub reproducer: ModelRoute,
    /// Memory-curator route.
    #[serde(default)]
    pub memory_curator: ModelRoute,
    /// Allow power routing for routine roles.
    #[serde(default)]
    pub allow_power_for_routine_roles: bool,
    /// Diversity pool for generative roles. When non-empty, generative lanes
    /// (hero / literature / brainstorm) round-robin across these routes by lane
    /// index so the tournament samples diverse models, while critic / verifier /
    /// meta roles keep their dedicated independent routes. Empty => no pooling
    /// (every lane of a role shares one route, as before).
    #[serde(default)]
    pub lane_diversity: Vec<ModelRoute>,
}

/// Whether a task kind is a generative role that should diversify across the
/// `lane_diversity` pool. Evaluative roles (critic / verifier / judge / meta /
/// red-team) deliberately do NOT diversify so they stay independent of the
/// generator pool.
fn kind_is_generative(kind: ModelTaskKind) -> bool {
    matches!(
        kind,
        ModelTaskKind::HeroGenerate
            | ModelTaskKind::LiteratureSynthesis
            | ModelTaskKind::StageBrainstorm
    )
}

impl ModelPolicy {
    /// Select a route for a workflow task kind.
    pub fn select(&self, kind: ModelTaskKind) -> ModelRouteRecord {
        let power = || self.power.normalized();
        let inherit_power_when_empty = |route: ModelRouteRecord| {
            if route.is_empty() {
                power()
            } else {
                route
            }
        };
        match kind {
            ModelTaskKind::Critic | ModelTaskKind::RedTeam => {
                inherit_power_when_empty(self.critic.normalized())
            }
            ModelTaskKind::Verifier => self.verifier.normalized(),
            ModelTaskKind::MetaJudge => inherit_power_when_empty(self.meta_judge.normalized()),
            ModelTaskKind::MemoryCurate | ModelTaskKind::KnowledgeCurate => {
                self.memory_curator.normalized()
            }
            ModelTaskKind::ParityGenerate => self.reproducer.normalized(),
            _ => {
                if self.allow_power_for_routine_roles || kind.uses_power_model() {
                    self.power.normalized()
                } else {
                    self.routine.normalized()
                }
            }
        }
    }

    /// Select a route for a specific lane index. Generative roles round-robin
    /// across the `lane_diversity` pool (when configured) for ensemble
    /// diversity; non-generative roles keep their dedicated route via
    /// [`select`](Self::select). Falls back to `select(kind)` when the pool is
    /// empty, so existing single-route behavior is unchanged.
    pub fn select_for_lane(&self, kind: ModelTaskKind, lane: usize) -> ModelRouteRecord {
        if self.lane_diversity.is_empty() || !kind_is_generative(kind) {
            return self.select(kind);
        }
        self.lane_diversity[lane % self.lane_diversity.len()].normalized()
    }
}

impl ModelTaskKind {
    /// Whether this task routes to the power model by default.
    pub fn uses_power_model(self) -> bool {
        match self {
            ModelTaskKind::StageBrainstorm
            | ModelTaskKind::StageReduce
            | ModelTaskKind::StageCritique
            | ModelTaskKind::Critic
            | ModelTaskKind::PerfClose
            | ModelTaskKind::HardEscalation
            | ModelTaskKind::PhaseFinalize
            | ModelTaskKind::StuckDebug
            | ModelTaskKind::Healing
            | ModelTaskKind::PerfGap
            | ModelTaskKind::Review
            | ModelTaskKind::RedTeam
            | ModelTaskKind::MetaJudge => true,
            ModelTaskKind::Frame
            | ModelTaskKind::PhaseBrainstorm
            | ModelTaskKind::Hypothesis
            | ModelTaskKind::Verifier
            | ModelTaskKind::MemoryCurate
            | ModelTaskKind::ParityGenerate
            | ModelTaskKind::Implement
            | ModelTaskKind::HeroGenerate
            | ModelTaskKind::JudgePatch
            | ModelTaskKind::LiteratureSynthesis
            | ModelTaskKind::KnowledgeCurate => false,
        }
    }
}

/// One model outcome receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelOutcome {
    /// Task id.
    pub task_id: String,
    /// Model id.
    pub model_id: String,
    /// Cost in USD.
    pub cost_usd: f64,
    /// Latency in milliseconds.
    pub latency_ms: u64,
    /// Whether the task succeeded.
    pub success: bool,
    /// Optional reviewer score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_score: Option<f64>,
    /// Whether this outcome became a winner.
    pub winner: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_policy_default_is_provider_neutral() {
        let policy = ModelPolicy::default();
        for kind in [
            ModelTaskKind::Implement,
            ModelTaskKind::Verifier,
            ModelTaskKind::StageBrainstorm,
            ModelTaskKind::Healing,
            ModelTaskKind::Review,
            ModelTaskKind::StageCritique,
            ModelTaskKind::Critic,
            ModelTaskKind::HardEscalation,
            ModelTaskKind::MetaJudge,
            ModelTaskKind::RedTeam,
            ModelTaskKind::HeroGenerate,
            ModelTaskKind::JudgePatch,
        ] {
            let route = policy.select(kind);
            assert_eq!(route.provider, None);
            assert_eq!(route.model, None);
        }
    }

    #[test]
    fn compact_route_string_splits_provider_and_model() {
        let policy = ModelPolicy {
            routine: ModelRoute::Compact("openrouter/qwen".to_string()),
            ..ModelPolicy::default()
        };
        let route = policy.select(ModelTaskKind::Implement);
        assert_eq!(route.provider.as_deref(), Some("openrouter"));
        assert_eq!(route.model.as_deref(), Some("qwen"));
    }

    #[test]
    fn quality_band_round_trips_through_role_selection() {
        // FIX-CAND-M: a per-role quality_band declared in the manifest
        // must survive policy.select() so jekko-runner can forward
        // it as JEKKO_RUN_QUALITY_BAND to the jekko-run subprocess.
        let policy = ModelPolicy {
            power: ModelRoute::Record(ModelRouteRecord {
                provider: None,
                model: None,
                quality_band: Some("top20".to_string()),
            }),
            ..ModelPolicy::default()
        };
        // StageBrainstorm is load-bearing for MiniRedis runs and must inherit
        // the power role's quality band.
        let route = policy.select(ModelTaskKind::StageBrainstorm);
        assert_eq!(route.quality_band.as_deref(), Some("top20"));
        // is_empty() must still consider a band-only record as non-empty
        // so the inherit_power_when_empty fallback fires correctly.
        let band_only = ModelRouteRecord {
            provider: None,
            model: None,
            quality_band: Some("top10".to_string()),
        };
        assert!(!band_only.is_empty());
    }

    #[test]
    fn select_for_lane_diversifies_generative_roles_only() {
        let policy = ModelPolicy {
            lane_diversity: vec![
                ModelRoute::Compact("provA/m1".to_string()),
                ModelRoute::Compact("provB/m2".to_string()),
            ],
            ..Default::default()
        };
        // Generative role (hero) round-robins the pool by lane index.
        let hero0 = policy.select_for_lane(ModelTaskKind::HeroGenerate, 0);
        let hero1 = policy.select_for_lane(ModelTaskKind::HeroGenerate, 1);
        let hero2 = policy.select_for_lane(ModelTaskKind::HeroGenerate, 2);
        assert_eq!(hero0.provider.as_deref(), Some("provA"));
        assert_eq!(hero1.provider.as_deref(), Some("provB"));
        assert_eq!(hero2.provider.as_deref(), Some("provA")); // wraps
        assert_ne!(hero0, hero1);
        // Evaluative role (verifier) ignores the pool and stays on its dedicated
        // route, independent of the generator pool.
        let verifier0 = policy.select_for_lane(ModelTaskKind::Verifier, 0);
        let verifier1 = policy.select_for_lane(ModelTaskKind::Verifier, 1);
        assert_eq!(verifier0, verifier1);
        assert_eq!(verifier0, policy.select(ModelTaskKind::Verifier));
    }

    #[test]
    fn select_for_lane_falls_back_to_select_when_pool_empty() {
        let policy = ModelPolicy::default();
        for lane in 0..4 {
            assert_eq!(
                policy.select_for_lane(ModelTaskKind::HeroGenerate, lane),
                policy.select(ModelTaskKind::HeroGenerate),
            );
        }
    }
}
