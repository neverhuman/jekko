use cogcore::core::{
    Core, FeedbackSignal, Intent, Outcome, RecallData, RecallQuery, Receipt, StoredEvent,
};

use crate::wal::{record_from_entry, WalOpDto};
use crate::{MemoryError, RecallAudit, TurnObservation, WalSink};

/// Tunables for the memory service.
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Token budget passed to `recall` when building a prompt context block.
    pub recall_token_budget: u32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            recall_token_budget: 1500,
        }
    }
}

/// Live memory service: a cogcore `Core` plus durable WAL persistence.
pub struct MemoryService<S: WalSink> {
    core: Core,
    sink: S,
    config: MemoryConfig,
    persisted_seq: u64,
}

impl<S: WalSink> MemoryService<S> {
    /// Construct a service over a fresh `Core` and the given sink. Call
    /// [`MemoryService::bootstrap`] to restore prior memory before serving.
    pub fn new(sink: S, config: MemoryConfig) -> Self {
        Self {
            core: Core::default(),
            sink,
            config,
            persisted_seq: 0,
        }
    }

    /// Restore memory from the sink on startup: validate the persisted WAL
    /// hash-chain, then replay the ops into a fresh `Core`.
    pub fn bootstrap(&mut self) -> Result<(), MemoryError> {
        let records = self.sink.load()?;
        if records.is_empty() {
            return Ok(());
        }

        let mut expected_prev = String::new();
        let mut expected_seq = 1_u64;
        for record in &records {
            if record.seq != expected_seq {
                return Err(MemoryError::SequenceBreak {
                    found: record.seq,
                    expected: expected_seq,
                });
            }
            if record.prev_hash != expected_prev {
                return Err(MemoryError::ChainBreak {
                    seq: record.seq,
                    found: record.prev_hash.clone(),
                    expected: expected_prev,
                });
            }
            expected_prev = record.hash.clone();
            expected_seq += 1;
        }

        let mut ops = Vec::with_capacity(records.len());
        for record in &records {
            let dto: WalOpDto = serde_json::from_str(&record.op_json)
                .map_err(|e| MemoryError::Serde(e.to_string()))?;
            ops.push(dto.into_op());
        }
        self.core = Core::from_wal_ops(ops);
        let rebuilt = self
            .core
            .wal_since(0)
            .iter()
            .map(record_from_entry)
            .collect::<Result<Vec<_>, _>>()?;
        if rebuilt.len() != records.len() {
            return Err(MemoryError::SequenceBreak {
                found: rebuilt.len() as u64,
                expected: records.len() as u64,
            });
        }
        for (record, rebuilt_record) in records.iter().zip(&rebuilt) {
            if record.prev_hash != rebuilt_record.prev_hash {
                return Err(MemoryError::ChainBreak {
                    seq: record.seq,
                    found: record.prev_hash.clone(),
                    expected: rebuilt_record.prev_hash.clone(),
                });
            }
            if record.hash != rebuilt_record.hash {
                return Err(MemoryError::HashMismatch {
                    seq: record.seq,
                    found: record.hash.clone(),
                    expected: rebuilt_record.hash.clone(),
                });
            }
        }
        self.persisted_seq = rebuilt.last().map(|record| record.seq).unwrap_or(0);
        Ok(())
    }

    /// Ingest one event into memory and persist the resulting WAL tail.
    pub fn observe(&mut self, event: StoredEvent) -> Result<Receipt, MemoryError> {
        let receipt = self.core.observe(event);
        self.persist_tail()?;
        Ok(receipt)
    }

    /// Ingest one chat turn into memory and persist the resulting WAL tail.
    pub fn observe_turn(&mut self, turn: TurnObservation) -> Result<Receipt, MemoryError> {
        self.observe(turn.into_stored_event())
    }

    /// Recall against memory. May emit a `RecallTouch` reinforcement op, whose
    /// WAL tail is persisted best-effort.
    pub fn recall(&mut self, query: &RecallQuery) -> RecallData {
        let data = self.core.recall(query);
        let _ = self.persist_tail();
        data
    }

    /// Build a recall context block for jekko's own session prompt assembly.
    /// Returns `None` when memory has nothing relevant. Never drives a runner.
    pub fn recall_block(&mut self, text: &str, token_budget: u32) -> Option<String> {
        let query = RecallQuery {
            text: text.to_string(),
            mentions: Vec::new(),
            intent: Intent::Recall,
            token_budget,
        };
        let data = self.recall(&query);
        if data.answer.trim().is_empty() {
            return None;
        }
        let mut block = String::from("=== RECALLED MEMORY (CONTEXT ONLY) ===\n");
        block.push_str(&data.answer);
        if !data.citations.is_empty() {
            block.push_str("\nSources:");
            for citation in &data.citations {
                block.push_str("\n- ");
                block.push_str(&citation.citation);
                block.push_str(" (");
                block.push_str(&citation.uri);
                block.push(')');
            }
        }
        Some(block)
    }

    /// Default-budget convenience over [`MemoryService::recall_block`].
    pub fn recall_block_default(&mut self, text: &str) -> Option<String> {
        self.recall_block(text, self.config.recall_token_budget)
    }

    /// Recall plus a [`RecallAudit`] record for the host to log.
    pub fn recall_audited(&mut self, query: &RecallQuery) -> (RecallData, RecallAudit) {
        let data = self.recall(query);
        let audit = RecallAudit::from_recall(&data);
        (data, audit)
    }

    /// Feed the reinforcement loop: record whether cited ids were used in the
    /// answer and persist the resulting `Feedback` WAL op.
    pub fn record_recall_outcome(
        &mut self,
        used_ids: &[String],
        used_in_answer: bool,
    ) -> Result<Option<Receipt>, MemoryError> {
        if used_ids.is_empty() {
            return Ok(None);
        }
        let outcome = if used_in_answer {
            Outcome::TaskSuccess
        } else {
            Outcome::Ignored
        };
        let signal = FeedbackSignal {
            outcome,
            used: used_ids.to_vec(),
        };
        let receipt = self.core.feedback(&signal);
        self.persist_tail()?;
        Ok(Some(receipt))
    }

    /// Borrow the underlying cogcore `Core`.
    pub fn core(&self) -> &Core {
        &self.core
    }

    /// Consume the service, returning the sink.
    pub fn into_sink(self) -> S {
        self.sink
    }

    fn persist_tail(&mut self) -> Result<(), MemoryError> {
        let tail = self.core.wal_since(self.persisted_seq);
        if tail.is_empty() {
            return Ok(());
        }
        let records = tail
            .iter()
            .map(record_from_entry)
            .collect::<Result<Vec<_>, _>>()?;
        self.sink.persist(&records)?;
        if let Some(last) = tail.last() {
            self.persisted_seq = last.seq;
        }
        Ok(())
    }
}
