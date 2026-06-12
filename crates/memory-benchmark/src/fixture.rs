//! 100 deterministic fixtures backing the Memory benchmark.
//!
//! This module defines the fixture schema and re-exports the generated corpus
//! from `fixture::data`.

use crate::{AxisScores, Domain, FixtureBlock, Pathology, PublicBench, RecallResult, TemporalLens};

/// A single deterministic test case.
///
/// Query is encoded as `(query_text, query_intent, query_mentions)` so the
/// entire Fixture remains const-constructible. The bench binary materializes
/// these into a `Query` struct at run time.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub id: u8, // 1..=100
    pub block: FixtureBlock,
    pub domain: Domain,
    pub pathologies: &'static [Pathology],
    pub requires_state_from: &'static [u8], // compounding predecessors
    pub setup: Setup,
    pub query_text: Option<&'static str>,
    pub query_intent: crate::QueryIntent,
    pub query_mentions: &'static [&'static str],
    pub lens: TemporalLens,
    pub world_time: Option<&'static str>,
    pub tx_time: Option<&'static str>,
    pub expected: Expected,
    pub public_bench: &'static [PublicBench],
    /// Pure function: candidate output → axis sub-scores for this fixture.
    pub grade: fn(&RecallResult, &Expected) -> AxisScores,
}

/// What the harness does before issuing `query` (if any).
#[derive(Debug, Clone)]
pub enum Setup {
    /// No setup — pure recall over previously-loaded state.
    NoSetup,
    /// Issue one or more observe() calls.
    Observe(&'static [SetupEvent]),
    /// Issue feedback against the most recent recall.
    Feedback {
        outcome_kind: &'static str, // "TaskSuccess" | "TaskFailure" | "Verified" | "Falsified"
        used_event_ids: &'static [&'static str],
        reason: &'static str,
    },
    /// Call rebuild() on the candidate.
    Rebuild,
    /// Call forget() against a memory id.
    Forget {
        memory_id: &'static str,
        reason: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct SetupEvent {
    pub id: &'static str,
    pub kind: &'static str, // matches EventKind name
    pub subject: &'static str,
    pub body: &'static str,
    pub source_uri: &'static str,
    pub source_citation: &'static str,
    pub source_quality: f32,
    pub valid_from: Option<&'static str>,
    pub valid_to: Option<&'static str>,
    pub tx_time: &'static str,
    pub privacy: &'static str, // "Public" | "Internal" | "Confidential" | "Secret" | "Vault"
    pub claim_modality: Option<&'static str>,
    pub tags: &'static [&'static str],
}

/// Ground truth for grading a single fixture.
#[derive(Debug, Clone, Default)]
pub struct Expected {
    /// IDs that MUST appear in `used_ids`.
    pub must_include: &'static [&'static str],
    /// IDs that MUST NOT appear (e.g. for `recall_as_of` causal mask).
    pub must_exclude: &'static [&'static str],
    /// Substrings that MUST appear in `answer` (case-insensitive contains).
    pub must_contain: &'static [&'static str],
    /// Substrings that MUST NOT appear (e.g. vault canaries).
    pub must_not_contain: &'static [&'static str],
    /// Warnings that MUST be present in the output.
    pub required_warnings: &'static [&'static str],
    /// At least one citation required (factual answer)?
    pub requires_citation: bool,
    /// Expected claim modality, if any.
    pub expected_modality: Option<&'static str>,
    /// Expected confidence range (low, high).
    pub confidence_range: Option<(f32, f32)>,
    /// For determinism fixtures: rebuild() must preserve export_state_hash().
    pub expects_stable_state_hash: bool,
}

/// The 100 fixtures. Populated by `fixture_data::FIXTURES`.
pub fn all() -> &'static [Fixture] {
    crate::fixture::data::FIXTURES.as_slice()
}

// Submodule holding the actual 100-entry static array.
pub mod data;
