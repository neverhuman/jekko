use cogcore::core::{ClaimModality, Intent, PrivacyClass, RecallQuery, SourceRef, StoredEvent};

use crate::{
    InMemoryWalSink, MemoryConfig, MemoryError, MemoryService, WalOpDto, WalRecord, WalSink,
};

fn ev(id: &str, subject: &str, body: &str, tx: &str) -> StoredEvent {
    StoredEvent {
        id: id.to_string(),
        kind: "Claim".to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
        tx_time: tx.to_string(),
        valid_from: Some("2020-01-01T00:00:00Z".to_string()),
        valid_to: None,
        privacy_class: PrivacyClass::Public,
        claim_modality: Some(ClaimModality::Observed),
        tags: Vec::new(),
        sources: vec![SourceRef {
            uri: "doi:example".to_string(),
            citation: "Example et al. 2024".to_string(),
            quality: 0.9,
        }],
        supersedes: Vec::new(),
        contradicts: Vec::new(),
    }
}

fn q(text: &str) -> RecallQuery {
    RecallQuery {
        text: text.to_string(),
        mentions: Vec::new(),
        intent: Intent::Recall,
        token_budget: 1024,
    }
}

#[test]
fn observe_then_recall_block_returns_memory() {
    let mut memory = MemoryService::new(InMemoryWalSink::default(), MemoryConfig::default());
    memory
        .observe(ev(
            "e1",
            "neutrino",
            "neutrinos have mass",
            "2020-01-01T00:00:00Z",
        ))
        .unwrap();
    let block = memory.recall_block("neutrino", 1024).unwrap();
    assert!(block.contains("=== RECALLED MEMORY (CONTEXT ONLY) ==="));
    assert!(block.contains("Example et al. 2024"));
}

#[test]
fn recall_block_is_none_when_memory_empty() {
    let mut memory = MemoryService::new(InMemoryWalSink::default(), MemoryConfig::default());
    assert!(memory.recall_block("anything", 512).is_none());
}

#[test]
fn recall_audited_captures_pack_and_used_ids() {
    let mut memory = MemoryService::new(InMemoryWalSink::default(), MemoryConfig::default());
    memory
        .observe(ev(
            "e1",
            "neutrino",
            "neutrinos have mass",
            "2020-01-01T00:00:00Z",
        ))
        .unwrap();
    let (data, audit) = memory.recall_audited(&q("neutrino"));
    assert_eq!(audit.pack_hash, data.context_pack_hash);
    assert!(audit.used_ids.contains(&"e1".to_string()));
}

#[test]
fn record_recall_outcome_reinforces_and_persists() {
    let mut memory = MemoryService::new(InMemoryWalSink::default(), MemoryConfig::default());
    memory
        .observe(ev(
            "e1",
            "neutrino",
            "neutrinos have mass",
            "2020-01-01T00:00:00Z",
        ))
        .unwrap();
    memory
        .observe(ev(
            "e2",
            "muon",
            "heavier than electron",
            "2020-01-01T00:00:00Z",
        ))
        .unwrap();
    let (_data, audit) = memory.recall_audited(&q("neutrino"));
    let before = memory.core().export_state_hash();

    let receipt = memory.record_recall_outcome(&audit.used_ids, true).unwrap();
    assert!(receipt.is_some(), "non-empty used_ids should feed feedback");
    assert_ne!(memory.core().export_state_hash(), before);

    let final_hash = memory.core().export_state_hash();
    let sink = memory.into_sink();
    let mut restored = MemoryService::new(sink, MemoryConfig::default());
    restored.bootstrap().unwrap();
    assert_eq!(restored.core().export_state_hash(), final_hash);
}

#[test]
fn record_recall_outcome_is_noop_for_empty_ids() {
    let mut memory = MemoryService::new(InMemoryWalSink::default(), MemoryConfig::default());
    assert!(memory.record_recall_outcome(&[], true).unwrap().is_none());
}

#[test]
fn persist_then_bootstrap_preserves_state_hash() {
    let mut first = MemoryService::new(InMemoryWalSink::default(), MemoryConfig::default());
    first
        .observe(ev(
            "e1",
            "neutrino",
            "mass is small",
            "2020-01-01T00:00:00Z",
        ))
        .unwrap();
    first
        .observe(ev(
            "e2",
            "neutrino",
            "ordering unknown",
            "2021-01-01T00:00:00Z",
        ))
        .unwrap();
    let _ = first.recall_block("neutrino", 1024);
    let state_hash = first.core().export_state_hash();
    let sink = first.into_sink();

    let mut restored = MemoryService::new(sink, MemoryConfig::default());
    restored.bootstrap().unwrap();
    assert_eq!(restored.core().export_state_hash(), state_hash);
}

#[test]
fn bootstrap_detects_chain_break() {
    let mut sink = InMemoryWalSink::default();
    sink.persist(&[
        WalRecord {
            seq: 1,
            prev_hash: String::new(),
            hash: "h1".to_string(),
            op_json: serde_json::to_string(&WalOpDto::Tombstone {
                event_id: "e1".to_string(),
                reason: "x".to_string(),
            })
            .unwrap(),
        },
        WalRecord {
            seq: 2,
            prev_hash: "WRONG".to_string(),
            hash: "h2".to_string(),
            op_json: serde_json::to_string(&WalOpDto::Tombstone {
                event_id: "e2".to_string(),
                reason: "y".to_string(),
            })
            .unwrap(),
        },
    ])
    .unwrap();
    let mut memory = MemoryService::new(sink, MemoryConfig::default());
    let err = memory.bootstrap().unwrap_err();
    assert!(matches!(err, MemoryError::ChainBreak { seq: 2, .. }));
}

#[test]
fn bootstrap_detects_sequence_break() {
    let mut sink = InMemoryWalSink::default();
    sink.persist(&[
        WalRecord {
            seq: 1,
            prev_hash: String::new(),
            hash: "h1".to_string(),
            op_json: serde_json::to_string(&WalOpDto::Tombstone {
                event_id: "e1".to_string(),
                reason: "x".to_string(),
            })
            .unwrap(),
        },
        WalRecord {
            seq: 3,
            prev_hash: "h1".to_string(),
            hash: "h3".to_string(),
            op_json: serde_json::to_string(&WalOpDto::Tombstone {
                event_id: "e3".to_string(),
                reason: "z".to_string(),
            })
            .unwrap(),
        },
    ])
    .unwrap();
    let mut memory = MemoryService::new(sink, MemoryConfig::default());
    let err = memory.bootstrap().unwrap_err();
    assert!(matches!(
        err,
        MemoryError::SequenceBreak {
            found: 3,
            expected: 2
        }
    ));
}

#[test]
fn bootstrap_detects_hash_mismatch() {
    let mut first = MemoryService::new(InMemoryWalSink::default(), MemoryConfig::default());
    first
        .observe(ev(
            "e1",
            "neutrino",
            "mass is small",
            "2020-01-01T00:00:00Z",
        ))
        .unwrap();
    let mut records = first.core().wal_since(0);
    let entry = records.pop().unwrap();
    let mut record = crate::wal::record_from_entry(&entry).unwrap();
    record.hash = "tampered".to_string();

    let mut sink = InMemoryWalSink::default();
    sink.persist(&[record]).unwrap();
    let mut memory = MemoryService::new(sink, MemoryConfig::default());
    let err = memory.bootstrap().unwrap_err();
    assert!(matches!(err, MemoryError::HashMismatch { seq: 1, .. }));
}
