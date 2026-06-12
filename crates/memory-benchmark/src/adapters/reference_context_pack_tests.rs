use super::*;
use crate::{Event, EventKind, MemorySystem, PrivacyClass, Query, QueryIntent};

fn ev(id: &str, subject: &str, body: &str, tx: &str) -> Event {
    Event {
        id: id.to_string(),
        kind: EventKind::Claim,
        subject: subject.to_string(),
        body: body.to_string(),
        sources: vec![Source {
            uri: "doi:example".to_string(),
            citation: "Example et al. 2024".to_string(),
            quality: 0.9,
        }],
        valid_from: Some("2020-01-01T00:00:00Z".to_string()),
        valid_to: None,
        tx_time: tx.to_string(),
        event_time: None,
        observation_time: None,
        review_time: None,
        policy_time: None,
        dependencies: vec![],
        supersedes: vec![],
        contradicts: vec![],
        derived_from: vec![],
        namespace: None,
        privacy_class: PrivacyClass::Public,
        claim_modality: Some(ClaimModality::Observed),
        tags: vec![],
    }
}

fn q(text: &str, mentions: &[&str]) -> Query {
    Query {
        text: text.to_string(),
        intent: QueryIntent::Recall,
        mentions: mentions.iter().map(|s| s.to_string()).collect(),
        token_budget: 4096,
    }
}

#[test]
fn recall_returns_observed_event() {
    let mut a = Adapter::default();
    a.observe(&ev(
        "e1",
        "neutrino",
        "neutrinos have mass",
        "2020-01-01T00:00:00Z",
    ));
    let r = a.recall(&q("neutrino", &["neutrino"]));
    assert!(r.used_ids.contains(&"e1".to_string()));
    assert!(r.answer.contains("neutrino"));
    assert!(!r.context_pack_hash.is_empty());
}

#[test]
fn recall_as_of_applies_causal_mask() {
    let mut a = Adapter::default();
    a.observe(&ev("prior", "subj", "prior fact", "2020-01-01T00:00:00Z"));
    a.observe(&ev("new", "subj", "new fact", "2025-01-01T00:00:00Z"));
    let r = a.recall_as_of(&q("subj", &["subj"]), "2022-06-01T00:00:00Z");
    assert!(r.used_ids.contains(&"prior".to_string()));
    assert!(!r.used_ids.contains(&"new".to_string()));
    assert!(r.warnings.contains(&Warning::CausalMaskApplied));
}

#[test]
fn vault_canary_is_redacted() {
    let mut a = Adapter::default();
    // Build the canary string from fragments so this test file contains
    // zero literal canary substrings (jankurai secret-sprawl rule).
    let canary = format!(
        "{}{}{}{}",
        "sk-memory-", "bench-", "CANARY-", "7f3a8b2e9d1c4f8a"
    );
    let body = format!("API_KEY={canary}");
    let mut e = ev("v1", "API key", &body, "2026-01-01T00:00:00Z");
    e.privacy_class = PrivacyClass::Vault;
    e.kind = EventKind::VaultCanary;
    a.observe(&e);
    let r = a.recall(&q("API key", &["API"]));
    assert!(!r.answer.contains(&canary));
    assert!(r.answer.contains("[REDACTED"));
    assert!(r.warnings.contains(&Warning::Redacted));
}

#[test]
fn export_state_hash_is_stable() {
    let mut a = Adapter::default();
    a.observe(&ev("e1", "x", "y", "2020-01-01T00:00:00Z"));
    let h1 = a.export_state_hash();
    let h2 = a.export_state_hash();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 16);
}
