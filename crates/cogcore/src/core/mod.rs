//! Cognitive memory core — Phase 2.
//!
//! Substantive memory engine: append-only WAL ledger, BM25-lite inverted
//! index, MinHash concept attachment, topic strength formula, Hebbian
//! co-activation matrix, FSRS-on-cells, and a `WalOp::RecallTouch` step
//! that records hot-path mutations so `rebuild()` is byte-identical.
//!
//! Public API surface unchanged from Phase 1 so the benchmark adapter
//! continues to work without modification.

use std::collections::BTreeMap;

use crate::budget::Budget;
use crate::concept::{Concept, ConceptId, Topic, TopicId};
use crate::config::DEFAULT_CITATION_QUALITY_FLOOR;
use crate::hash::{fnv1a_hex, fnv1a_seq_hex};
use crate::hebb::Hebb;
use crate::index::{Interner, InvertedIndex, TokenId};
use crate::ledger::Wal;
use crate::time::BENCH_NOW;

mod consolidate;
mod observe;
mod recall;
mod recall_render;
mod state;
mod support;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyClass {
    Public,
    Internal,
    Confidential,
    Secret,
    Vault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimModality {
    Observed,
    AssertedBySource,
    InferredByAgent,
    HumanApproved,
    FormallyVerified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreWarning {
    Superseded,
    Contradicted,
    Redacted,
    CausalMaskApplied,
    SkeptikSurfaced,
    UnitMismatch,
    Abstained,
    UnsafeToolRefused,
}

pub use CoreWarning as Warning;

#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub body: String,
    pub tx_time: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub privacy_class: PrivacyClass,
    pub claim_modality: Option<ClaimModality>,
    pub tags: Vec<String>,
    pub sources: Vec<SourceRef>,
    pub supersedes: Vec<String>,
    pub contradicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceRef {
    pub uri: String,
    pub citation: String,
    pub quality: f32,
}

#[derive(Debug, Clone)]
pub struct Receipt {
    pub event_id: Option<String>,
    pub mutation_id: String,
    pub at: String,
    pub previous_hash: String,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct Tombstone {
    pub memory_id: String,
    pub reason: String,
    pub deletion_proof: String,
    pub deleted_at: String,
}

#[derive(Debug, Clone)]
pub struct FeedbackSignal {
    pub outcome: Outcome,
    pub used: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreOutcome {
    TaskSuccess,
    TaskFailure,
    Verified,
    Falsified,
    Ignored,
}

pub use CoreOutcome as Outcome;

#[derive(Debug, Clone)]
pub struct CitedSource {
    pub uri: String,
    pub citation: String,
}

#[derive(Debug, Clone, Default)]
pub struct RecallData {
    pub answer: String,
    pub citations: Vec<CitedSource>,
    pub warnings: Vec<Warning>,
    pub used_ids: Vec<String>,
    pub confidence: f32,
    pub context_pack_hash: String,
    pub claim_modality: Option<ClaimModality>,
    pub omitted_bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Intent {
    #[default]
    Recall,
    Fact,
    Equation,
    Theorem,
    Citation,
    Coref,
    Procedure,
    Workflow,
    Contradiction,
    HistoryAt,
    HistoryAsOf,
    Forget,
    Mixed,
}

#[derive(Debug, Clone)]
pub struct RecallQuery {
    pub text: String,
    pub mentions: Vec<String>,
    pub intent: Intent,
    pub token_budget: u32,
}

/// Internal cell representation. The interned `tokens` and `bigrams`
/// support BM25 + MinHash without re-tokenizing on recall.
pub(super) struct Cell {
    pub(super) event: StoredEvent,
    pub(super) tokens: Vec<TokenId>,
    pub(super) sketch: [u32; 8],
    pub(super) strength: f32,
    pub(super) half_life_hours: f32,
    pub(super) last_recall_tx: String,
    pub(super) recall_count: u32,
    pub(super) success_count: u32,
    pub(super) failure_count: u32,
    pub(super) utility: f32,
    pub(super) concept_id: Option<ConceptId>,
}

pub struct Core {
    pub(super) cells: Vec<Cell>,
    pub(super) by_id: BTreeMap<String, u32>,
    pub(super) exact_id_index: BTreeMap<String, u32>,
    pub(super) subject_index: BTreeMap<String, Vec<u32>>,
    pub(super) equation_lane: BTreeMap<String, Vec<u32>>,
    pub(super) theorem_lane: BTreeMap<String, Vec<u32>>,
    pub(super) tombstones: BTreeMap<String, Tombstone>,
    pub(super) interner: Interner,
    pub(super) index: InvertedIndex,
    pub(super) hebb: Hebb,
    pub(super) concepts: Vec<Concept>,
    pub(super) topics: Vec<Topic>,
    pub(super) topic_lookup: BTreeMap<String, TopicId>,
    pub(super) receipt_seq: u64,
    pub(super) last_receipt_hash: String,
    pub(super) citation_quality_floor: f32,
    pub(super) wal: Wal,
    pub(super) consolidation_budget: Budget,
}

impl Default for Core {
    fn default() -> Self {
        Core {
            cells: Vec::new(),
            by_id: BTreeMap::new(),
            exact_id_index: BTreeMap::new(),
            subject_index: BTreeMap::new(),
            equation_lane: BTreeMap::new(),
            theorem_lane: BTreeMap::new(),
            tombstones: BTreeMap::new(),
            interner: Interner::default(),
            index: InvertedIndex::default(),
            hebb: Hebb::default(),
            concepts: Vec::new(),
            topics: Vec::new(),
            topic_lookup: BTreeMap::new(),
            receipt_seq: 0,
            last_receipt_hash: String::new(),
            citation_quality_floor: DEFAULT_CITATION_QUALITY_FLOOR,
            wal: Wal::default(),
            consolidation_budget: Budget::ZERO,
        }
    }
}

impl Core {
    pub fn with_citation_quality_floor(citation_quality_floor: f32) -> Self {
        Core {
            citation_quality_floor,
            ..Core::default()
        }
    }

    /// Configure the LLM/embedding budget consulted by future
    /// `consolidate()` calls. Defaults to [`Budget::ZERO`], which keeps
    /// cogcore on the deterministic rule-based path. Hosts that wire a
    /// non-default `ConsolidationBackend` (e.g. ZYAL-mediated Jnoccio)
    /// can override this before running consolidation.
    pub fn set_consolidation_budget(&mut self, budget: Budget) {
        self.consolidation_budget = budget;
    }

    pub fn canonical_event_id(kind: &str, subject: &str, body: &str, tx_time: &str) -> String {
        fnv1a_seq_hex(&[kind, subject, body, tx_time])
    }

    pub(super) fn next_receipt(&mut self, event_id: Option<&str>, kind: &str) -> Receipt {
        self.receipt_seq += 1;
        let prev = self.last_receipt_hash.clone();
        let hash = fnv1a_hex(&format!("{}:{}:{}", prev, self.receipt_seq, kind));
        self.last_receipt_hash = hash.clone();
        Receipt {
            event_id: event_id.map(|s| s.to_string()),
            mutation_id: format!("cogcore-{:08}", self.receipt_seq),
            at: BENCH_NOW.to_string(),
            previous_hash: prev,
            hash,
        }
    }

    pub fn concepts(&self) -> &[Concept] {
        &self.concepts
    }
    pub fn topics(&self) -> &[Topic] {
        &self.topics
    }
    pub fn wal_len(&self) -> usize {
        self.wal.len()
    }
    pub fn topic_count(&self) -> usize {
        self.topics.len()
    }

    /// Programmatic helper: open an explicit topic for tests.
    #[doc(hidden)]
    pub fn debug_open_topic(&mut self, label: &str) -> TopicId {
        let id = self.topics.len() as TopicId;
        self.topics.push(Topic {
            id,
            label: label.to_string(),
            concepts: Vec::new(),
            strength: 0.5,
            half_life_hours: 24.0,
            last_update_tx: BENCH_NOW.to_string(),
            contradiction_pressure: 0.0,
            stats: crate::topic::empty_stats(),
        });
        self.topic_lookup.insert(consolidate::topic_key(label), id);
        id
    }
}

pub use support::pack_hash;
pub(super) use support::{
    modality_byte, modality_from_byte, outcome_byte, outcome_from_byte, privacy_byte,
    privacy_from_byte, push_unique,
};

#[cfg(test)]
mod tests;
