//! `Core::consolidate` — Hebbian pruning, concept formation, and topic
//! emergence. The standalone helpers (`cell_concept_sort_key`, `topic_key`,
//! `topic_label`, `topic_coactivation`, `concept_pair_weight`, `topic_stats`,
//! `topic_contradiction_pressure`) live here because they exist solely to
//! drive consolidation; the recall path does not need them.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use super::Core;
use crate::concept::{formation_threshold, Concept, ConceptId, Topic, TopicId};
use crate::config::{CONCEPT_CONFLICT_THRESHOLD, CONCEPT_KERNEL_LIMIT, CONCEPT_MIN_MEMBERS};
use crate::index::{tokenize, TokenId};
use crate::time::BENCH_NOW;
use crate::topic::recompute as topic_recompute;

/// Whether a consolidation pass is due: at least `every` events have entered the
/// WAL since the last consolidation. Pure and deterministic so callers can own
/// the cadence state directly. `every == 0` means "never".
pub fn consolidation_due(wal_len: usize, last_consolidated_wal: usize, every: usize) -> bool {
    every > 0 && wal_len.saturating_sub(last_consolidated_wal) >= every
}

impl Core {
    /// Run consolidation if at least `every` events have entered the WAL since
    /// the last consolidation, then advance the cadence cursor. Returns whether
    /// it consolidated. Gives long-running agents an automatic compounding
    /// cadence instead of leaving `consolidate()` to be called manually (or
    /// never) — the scheduling gap noted in U9. `every == 0` disables it.
    pub fn consolidate_if_due(&mut self, every: usize) -> bool {
        if !consolidation_due(self.wal_len(), self.last_consolidated_wal, every) {
            return false;
        }
        self.consolidate();
        self.last_consolidated_wal = self.wal_len();
        true
    }

    pub fn consolidate(&mut self) {
        self.hebb.prune();
        let unprocessed: Vec<u32> = self
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.concept_id.is_none())
            .map(|(i, _)| i as u32)
            .collect();
        let mut buckets: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for idx in unprocessed {
            if let Some(c) = self.cells.get(idx as usize) {
                buckets.entry(c.sketch[0]).or_default().push(idx);
            }
        }
        for (_bucket_key, mut members) in buckets {
            if members.len() < CONCEPT_MIN_MEMBERS {
                continue;
            }
            members.sort_by(|a, b| {
                cell_concept_sort_key(self, *a).cmp(&cell_concept_sort_key(self, *b))
            });
            let representative = members[0] as usize;
            // The bucket was built from the live cell list. A missing
            // representative means consolidation is racing with a tombstone,
            // so skip this bucket; the next consolidation cycle will rebuild it.
            let Some(rep_cell) = self.cells.get(representative) else {
                continue;
            };
            let label = rep_cell.event.subject.clone();
            let kernel: Vec<TokenId> = rep_cell
                .tokens
                .iter()
                .take(CONCEPT_KERNEL_LIMIT)
                .copied()
                .collect();
            let sketch = rep_cell.sketch;
            let cluster_quality = if members.is_empty() {
                0.0
            } else {
                let mut total = 0.0_f32;
                let mut counted = 0_u32;
                for idx in &members {
                    if let Some(cell) = self.cells.get(*idx as usize) {
                        total += crate::index::jaccard_minhash(&sketch, &cell.sketch);
                        counted += 1;
                    }
                }
                if counted == 0 {
                    0.0
                } else {
                    total / counted as f32
                }
            };
            if cluster_quality < formation_threshold() {
                continue;
            }
            let id = self.concepts.len() as ConceptId;
            self.concepts.push(Concept {
                id,
                label,
                kernel_tokens: kernel,
                minhash: sketch,
                member_cells: members.clone(),
            });
            for m in members {
                if let Some(c) = self.cells.get_mut(m as usize) {
                    c.concept_id = Some(id);
                }
            }
        }

        let mut topic_groups: BTreeMap<String, Vec<ConceptId>> = BTreeMap::new();
        for concept in &self.concepts {
            topic_groups
                .entry(topic_key(&concept.label))
                .or_default()
                .push(concept.id);
        }

        let mut topic_lookup = self.topic_lookup.clone();
        for (key, concept_ids) in topic_groups {
            let coactivation = topic_coactivation(self, &concept_ids);
            if concept_ids.len() < 2 && coactivation < crate::config::TOPIC_EMERGENCE_WEIGHT {
                continue;
            }
            let topic_id = if let Some(existing) = self.topic_lookup.get(&key).copied() {
                existing
            } else {
                let id = self.topics.len() as TopicId;
                self.topics.push(Topic {
                    id,
                    label: key.clone(),
                    concepts: Vec::new(),
                    strength: 0.5,
                    half_life_hours: 24.0,
                    last_update_tx: BENCH_NOW.to_string(),
                    contradiction_pressure: 0.0,
                    stats: crate::topic::empty_stats(),
                });
                id
            };
            let topic_label_value = topic_label(self, &concept_ids);
            let topic_stats_value = topic_stats(self, &concept_ids);
            let topic_pressure = topic_contradiction_pressure(self, &concept_ids, coactivation);
            let topic = self
                .topics
                .iter_mut()
                .find(|topic| topic.id == topic_id)
                .expect("topic id must exist");
            topic.label = topic_label_value;
            topic.concepts = concept_ids.clone();
            topic.stats = topic_stats_value;
            topic.strength = (topic.strength
                + coactivation * crate::config::TOPIC_EMERGENCE_WEIGHT)
                .clamp(0.0, 1.0);
            topic.contradiction_pressure = topic_pressure;
            topic_recompute(topic, BENCH_NOW);
            topic_lookup.insert(key, topic_id);
        }
        self.topic_lookup = topic_lookup;
        for topic in self.topics.iter_mut() {
            topic_recompute(topic, BENCH_NOW);
        }

        // Phase 8+: invoke ConsolidationBackend::summarize_topic here when a
        // non-zero budget is configured. RuleBackend in `consolidate.rs` ships
        // the rule-based scaffold; JnoccioBackend is deferred per AGENT_CHAT
        // (no Rust SDK; future ZYAL workflow drives via cogcore_bench).
        if self.consolidation_budget.has_any_llm() {
            // No backend wired yet — budget is threaded only so future
            // hosts can drive an LLM-enrich pass without ABI churn.
        }
    }
}

fn cell_concept_sort_key(core: &Core, idx: u32) -> (Reverse<i64>, String, String, String) {
    let cell = core
        .cells
        .get(idx as usize)
        .expect("cell index must exist during consolidation");
    (
        Reverse((cell.strength * 1000.0).round() as i64),
        cell.event.subject.clone(),
        cell.event.body.clone(),
        cell.event.id.clone(),
    )
}

pub(super) fn topic_key(label: &str) -> String {
    match tokenize(label).into_iter().next() {
        Some(token) => token,
        None => label.to_ascii_lowercase(),
    }
}

fn topic_label(core: &Core, concept_ids: &[ConceptId]) -> String {
    let mut labels: Vec<String> = concept_ids
        .iter()
        .filter_map(|id| core.concepts.iter().find(|concept| concept.id == *id))
        .map(|concept| concept.label.clone())
        .collect();
    labels.sort();
    match labels.into_iter().next() {
        Some(name) => name,
        None => String::from("topic"),
    }
}

fn topic_coactivation(core: &Core, concept_ids: &[ConceptId]) -> f32 {
    let mut total = 0.0_f32;
    let mut pairs = 0u32;
    for (i, left_id) in concept_ids.iter().enumerate() {
        for right_id in &concept_ids[i + 1..] {
            total += concept_pair_weight(core, *left_id, *right_id);
            pairs += 1;
        }
    }
    if pairs == 0 {
        0.0
    } else {
        (total / pairs as f32).clamp(0.0, 1.0)
    }
}

fn concept_pair_weight(core: &Core, a: ConceptId, b: ConceptId) -> f32 {
    let Some(left) = core.concepts.iter().find(|concept| concept.id == a) else {
        return 0.0;
    };
    let Some(right) = core.concepts.iter().find(|concept| concept.id == b) else {
        return 0.0;
    };
    let mut total = 0.0_f32;
    let mut pairs = 0u32;
    for left_cell in &left.member_cells {
        for right_cell in &right.member_cells {
            total += core.hebb.weight(*left_cell, *right_cell);
            pairs += 1;
        }
    }
    if pairs == 0 {
        0.0
    } else {
        (total / pairs as f32).clamp(0.0, 1.0)
    }
}

fn topic_stats(core: &Core, concept_ids: &[ConceptId]) -> crate::concept::TopicStats {
    let mut stats = crate::concept::TopicStats::default();
    let mut subjects = BTreeSet::new();
    let mut source_quality_total = 0.0_f32;
    let mut source_quality_count = 0u32;
    for concept_id in concept_ids {
        if let Some(concept) = core
            .concepts
            .iter()
            .find(|concept| concept.id == *concept_id)
        {
            subjects.insert(concept.label.to_ascii_lowercase());
            for cell_idx in &concept.member_cells {
                if let Some(cell) = core.cells.get(*cell_idx as usize) {
                    stats.recall_count = stats.recall_count.saturating_add(cell.recall_count);
                    stats.success_count = stats.success_count.saturating_add(cell.success_count);
                    stats.failure_count = stats.failure_count.saturating_add(cell.failure_count);
                    stats.recent_observes = stats.recent_observes.saturating_add(1);
                    let src_q = cell
                        .event
                        .sources
                        .iter()
                        .map(|src| src.quality)
                        .fold(0.0_f32, f32::max);
                    source_quality_total += src_q;
                    source_quality_count = source_quality_count.saturating_add(1);
                }
            }
        }
    }
    stats.distinct_subjects = subjects.len() as u32;
    stats.avg_source_quality = if source_quality_count == 0 {
        0.0
    } else {
        source_quality_total / source_quality_count as f32
    };
    stats
}

fn topic_contradiction_pressure(core: &Core, concept_ids: &[ConceptId], coactivation: f32) -> f32 {
    let mut pressure = (1.0 - coactivation).clamp(0.0, 1.0);
    if pressure >= CONCEPT_CONFLICT_THRESHOLD {
        pressure = (pressure + 0.05).clamp(0.0, 1.0);
    }
    for concept_id in concept_ids {
        if let Some(concept) = core
            .concepts
            .iter()
            .find(|concept| concept.id == *concept_id)
        {
            for cell_idx in &concept.member_cells {
                if let Some(cell) = core.cells.get(*cell_idx as usize) {
                    if !cell.event.contradicts.is_empty() || !cell.event.supersedes.is_empty() {
                        pressure = (pressure + 0.10).clamp(0.0, 1.0);
                    }
                }
            }
        }
    }
    pressure
}
