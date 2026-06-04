//! State-management primitives: WAL replay (`rebuild`), recall-touch replay,
//! deterministic state hashing, and the rough byte accountant.

use super::{
    modality_from_byte, outcome_from_byte, privacy_from_byte, Core, FeedbackSignal, Receipt,
    SourceRef, StoredEvent,
};
use crate::fsrs::{decay as fsrs_decay, hours_between, strengthen_cell};
use crate::hash::fnv1a_hex;
use crate::ledger::WalOp;
use crate::time::BENCH_NOW;

impl Core {
    /// Build a `Core` from a persisted WAL op list — e.g. rows loaded from a
    /// host's `cogcore_wal` table on restart. The ops are appended to a fresh
    /// WAL (reproducing the FNV-1a hash chain) and replayed via [`Core::rebuild`],
    /// so the resulting `export_state_hash()` matches the original byte-for-byte
    /// (cogcore's determinism contract). This is the bootstrap counterpart to
    /// [`Core::wal_entries`] / [`Core::wal_since`].
    pub fn from_wal_ops(ops: Vec<WalOp>) -> Core {
        let mut core = Core::default();
        for op in ops {
            core.wal.append(op);
        }
        core.rebuild();
        core
    }

    /// Replay the WAL into a fresh in-memory state. Byte-identical to live
    /// state if no clock/random has been touched on the hot path.
    pub fn rebuild(&mut self) -> Receipt {
        // Snapshot WAL ops, then rebuild from scratch.
        let snapshot: Vec<WalOp> = self.wal.entries().iter().map(|e| e.op.clone()).collect();
        let old_seq = self.receipt_seq;
        let old_last = self.last_receipt_hash.clone();
        let old_floor = self.citation_quality_floor;
        *self = Core {
            citation_quality_floor: old_floor,
            ..Core::default()
        };
        for op in snapshot {
            match op {
                WalOp::Observe {
                    event_id,
                    kind,
                    subject,
                    body,
                    tx_time,
                    valid_from,
                    valid_to,
                    privacy_class,
                    claim_modality,
                    tags,
                    sources,
                    supersedes,
                    contradicts,
                } => {
                    let ev = StoredEvent {
                        id: event_id,
                        kind,
                        subject,
                        body,
                        tx_time,
                        valid_from,
                        valid_to,
                        privacy_class: privacy_from_byte(privacy_class),
                        claim_modality: claim_modality.map(modality_from_byte),
                        tags,
                        sources: sources
                            .into_iter()
                            .map(|(uri, citation, quality)| SourceRef {
                                uri,
                                citation,
                                quality,
                            })
                            .collect(),
                        supersedes,
                        contradicts,
                    };
                    let _ = self.observe(ev);
                }
                WalOp::Tombstone { event_id, reason } => {
                    let _ = self.forget(&event_id, &reason);
                }
                WalOp::Feedback { outcome, used } => {
                    let sig = FeedbackSignal {
                        outcome: outcome_from_byte(outcome),
                        used,
                    };
                    let _ = self.feedback(&sig);
                }
                WalOp::RecallTouch { used_ids, tx_time } => {
                    self.wal.append(WalOp::RecallTouch {
                        used_ids: used_ids.clone(),
                        tx_time: tx_time.clone(),
                    });
                    self.apply_recall_touch(&used_ids, &tx_time);
                }
            }
        }
        self.receipt_seq = old_seq + 1;
        let prev = old_last;
        let hash = fnv1a_hex(&format!("{}:{}:{}", prev, self.receipt_seq, "rebuild"));
        self.last_receipt_hash = hash.clone();
        Receipt {
            event_id: None,
            mutation_id: format!("cogcore-{:08}", self.receipt_seq),
            at: BENCH_NOW.to_string(),
            previous_hash: prev,
            hash,
        }
    }

    /// Apply the deterministic mutations recorded by a `RecallTouch` op.
    /// Called both on the hot path (after a successful recall) and during
    /// `rebuild()` so live state and replayed state converge.
    pub(super) fn apply_recall_touch(&mut self, used_ids: &[String], tx_time: &str) {
        let mut indices: Vec<u32> = Vec::new();
        for id in used_ids {
            if let Some(idx) = self.by_id.get(id).copied() {
                indices.push(idx);
                if let Some(cell) = self.cells.get_mut(idx as usize) {
                    cell.recall_count = cell.recall_count.saturating_add(1);
                    let success_rate = cell.success_count as f32
                        / (cell.success_count + cell.failure_count + 1) as f32;
                    let dt_h = hours_between(&cell.last_recall_tx, tx_time);
                    let half = cell.half_life_hours.max(1.0);
                    let decayed = fsrs_decay(cell.strength, dt_h, half);
                    let src_q = cell
                        .event
                        .sources
                        .iter()
                        .map(|s| s.quality)
                        .fold(0.0_f32, f32::max);
                    cell.strength = strengthen_cell(decayed, success_rate, src_q, cell.utility);
                    cell.half_life_hours = crate::fsrs::cell_half_life_hours(
                        cell.strength,
                        success_rate,
                        cell.recall_count,
                    );
                    cell.last_recall_tx = tx_time.to_string();
                }
            }
        }
        self.hebb.update_recall(&indices);
    }

    /// Compute a state hash that is invariant under insertion order.
    pub fn export_state_hash(&self) -> String {
        let mut buf = String::new();
        let mut ids: Vec<&str> = self.by_id.keys().map(|s| s.as_str()).collect();
        ids.sort();
        for id in &ids {
            buf.push_str(id);
            buf.push('|');
        }
        let mut idx_pairs: Vec<(String, f32, f32, u32, u32, u32)> = self
            .cells
            .iter()
            .map(|c| {
                (
                    c.event.id.clone(),
                    c.utility,
                    c.strength,
                    c.recall_count,
                    c.success_count,
                    c.failure_count,
                )
            })
            .collect();
        idx_pairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (id, u, s, r, sc, fc) in idx_pairs {
            buf.push_str(&id);
            buf.push(':');
            buf.push_str(&format!("u={u:.4},s={s:.4},r={r},sc={sc},fc={fc};"));
        }
        for (id, idx) in &self.exact_id_index {
            buf.push_str(&format!("E:{id}={idx};"));
        }
        for (subject, ids) in &self.subject_index {
            buf.push_str(&format!("S:{subject}={:?};", ids));
        }
        for (subject, ids) in &self.equation_lane {
            buf.push_str(&format!("Q:{subject}={:?};", ids));
        }
        for (subject, ids) in &self.theorem_lane {
            buf.push_str(&format!("T:{subject}={:?};", ids));
        }
        for (topic_key, topic_id) in &self.topic_lookup {
            buf.push_str(&format!("K:{topic_key}={topic_id};"));
        }
        for concept in &self.concepts {
            buf.push_str(&format!(
                "C:{}:{}:{:?}:{:?};",
                concept.id, concept.label, concept.kernel_tokens, concept.member_cells
            ));
        }
        for topic in &self.topics {
            buf.push_str(&format!(
                "P:{}:{}:{:?}:{:.4}:{:.4}:{:.4};",
                topic.id,
                topic.label,
                topic.concepts,
                topic.strength,
                topic.half_life_hours,
                topic.contradiction_pressure
            ));
        }
        for ((a, b), w) in self.hebb.edges_sorted() {
            buf.push_str(&format!("C:{a}-{b}={w:.4};"));
        }
        for k in self.tombstones.keys() {
            buf.push_str("T:");
            buf.push_str(k);
            buf.push(';');
        }
        fnv1a_hex(&buf)
    }

    pub fn state_bytes(&self) -> u64 {
        let mut total: u64 = 0;
        for c in &self.cells {
            total = total.saturating_add(c.event.body.len() as u64);
            total = total.saturating_add(c.event.subject.len() as u64);
            total = total.saturating_add((c.tokens.len() * 4) as u64);
        }
        total.saturating_add((self.hebb.len() * 12) as u64)
    }
}
