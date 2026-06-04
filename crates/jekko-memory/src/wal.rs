use cogcore::ledger::{WalEntry, WalOp};
use serde::{Deserialize, Serialize};

use crate::MemoryError;

/// A persisted WAL record: the cogcore hash-chain fields plus the op as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalRecord {
    /// 1-based sequence, matches [`WalEntry::seq`].
    pub seq: u64,
    /// Hash of the preceding entry, empty for seq 1.
    pub prev_hash: String,
    /// FNV-1a chain hash of this entry.
    pub hash: String,
    /// The WAL op serialized through [`WalOpDto`].
    pub op_json: String,
}

/// Durable sink for the cogcore WAL. Implemented by the host over SQLite;
/// `jekko-memory` stays storage-agnostic.
pub trait WalSink {
    /// Append persisted WAL records. Hosts should commit the batch atomically.
    fn persist(&mut self, records: &[WalRecord]) -> Result<(), MemoryError>;
    /// Load all persisted records in `seq` order for bootstrap.
    fn load(&self) -> Result<Vec<WalRecord>, MemoryError>;
}

/// In-memory [`WalSink`] for tests and broker-free defaults. Not durable across
/// process restart.
#[derive(Debug, Default)]
pub struct InMemoryWalSink {
    records: Vec<WalRecord>,
}

impl WalSink for InMemoryWalSink {
    fn persist(&mut self, records: &[WalRecord]) -> Result<(), MemoryError> {
        self.records.extend_from_slice(records);
        Ok(())
    }

    fn load(&self) -> Result<Vec<WalRecord>, MemoryError> {
        Ok(self.records.clone())
    }
}

/// Serde mirror of [`WalOp`]. cogcore intentionally stays zero-dependency, so
/// this crate owns the serialization boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
#[allow(clippy::large_enum_variant)]
pub enum WalOpDto {
    /// See [`WalOp::Observe`].
    Observe {
        #[allow(missing_docs)]
        event_id: String,
        #[allow(missing_docs)]
        kind: String,
        #[allow(missing_docs)]
        subject: String,
        #[allow(missing_docs)]
        body: String,
        #[allow(missing_docs)]
        tx_time: String,
        #[allow(missing_docs)]
        valid_from: Option<String>,
        #[allow(missing_docs)]
        valid_to: Option<String>,
        #[allow(missing_docs)]
        privacy_class: u8,
        #[allow(missing_docs)]
        claim_modality: Option<u8>,
        #[allow(missing_docs)]
        tags: Vec<String>,
        #[allow(missing_docs)]
        sources: Vec<(String, String, f32)>,
        #[allow(missing_docs)]
        supersedes: Vec<String>,
        #[allow(missing_docs)]
        contradicts: Vec<String>,
    },
    /// See [`WalOp::Tombstone`].
    Tombstone {
        #[allow(missing_docs)]
        event_id: String,
        #[allow(missing_docs)]
        reason: String,
    },
    /// See [`WalOp::Feedback`].
    Feedback {
        #[allow(missing_docs)]
        outcome: u8,
        #[allow(missing_docs)]
        used: Vec<String>,
    },
    /// See [`WalOp::RecallTouch`].
    RecallTouch {
        #[allow(missing_docs)]
        used_ids: Vec<String>,
        #[allow(missing_docs)]
        tx_time: String,
    },
}

impl WalOpDto {
    pub(crate) fn from_op(op: &WalOp) -> Self {
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
            } => WalOpDto::Observe {
                event_id: event_id.clone(),
                kind: kind.clone(),
                subject: subject.clone(),
                body: body.clone(),
                tx_time: tx_time.clone(),
                valid_from: valid_from.clone(),
                valid_to: valid_to.clone(),
                privacy_class: *privacy_class,
                claim_modality: *claim_modality,
                tags: tags.clone(),
                sources: sources.clone(),
                supersedes: supersedes.clone(),
                contradicts: contradicts.clone(),
            },
            WalOp::Tombstone { event_id, reason } => WalOpDto::Tombstone {
                event_id: event_id.clone(),
                reason: reason.clone(),
            },
            WalOp::Feedback { outcome, used } => WalOpDto::Feedback {
                outcome: *outcome,
                used: used.clone(),
            },
            WalOp::RecallTouch { used_ids, tx_time } => WalOpDto::RecallTouch {
                used_ids: used_ids.clone(),
                tx_time: tx_time.clone(),
            },
        }
    }

    pub(crate) fn into_op(self) -> WalOp {
        match self {
            WalOpDto::Observe {
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
            } => WalOp::Observe {
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
            },
            WalOpDto::Tombstone { event_id, reason } => WalOp::Tombstone { event_id, reason },
            WalOpDto::Feedback { outcome, used } => WalOp::Feedback { outcome, used },
            WalOpDto::RecallTouch { used_ids, tx_time } => WalOp::RecallTouch { used_ids, tx_time },
        }
    }
}

pub(crate) fn record_from_entry(entry: &WalEntry) -> Result<WalRecord, MemoryError> {
    let dto = WalOpDto::from_op(&entry.op);
    let op_json = serde_json::to_string(&dto).map_err(|e| MemoryError::Serde(e.to_string()))?;
    Ok(WalRecord {
        seq: entry.seq,
        prev_hash: entry.prev_hash.clone(),
        hash: entry.hash.clone(),
        op_json,
    })
}
