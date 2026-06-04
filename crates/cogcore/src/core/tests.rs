//! End-to-end behavioural tests for `Core`. The cases cross multiple
//! submodules (observe + recall + state + consolidate) so the tests live
//! alongside `mod.rs` rather than inside any single submodule.

use super::*;

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
        mentions: vec![text.to_string()],
        intent: Intent::Recall,
        token_budget: 4096,
    }
}

#[test]
fn from_wal_ops_roundtrip_preserves_state_hash() {
    let mut c = Core::default();
    c.observe(ev(
        "e1",
        "neutrino",
        "neutrinos have mass",
        "2020-01-01T00:00:00Z",
    ));
    c.observe(ev(
        "e2",
        "neutrino",
        "mass ordering unknown",
        "2021-01-01T00:00:00Z",
    ));
    let _ = c.recall(&q("neutrino")); // emits a RecallTouch op into the WAL
    let before = c.export_state_hash();

    // Simulate persist -> restart: pull the WAL op stream, rebuild a fresh Core.
    let ops: Vec<_> = c.wal_entries().iter().map(|e| e.op.clone()).collect();
    let loaded = Core::from_wal_ops(ops);

    assert_eq!(loaded.export_state_hash(), before);
}

#[test]
fn wal_since_returns_only_new_tail() {
    let mut c = Core::default();
    c.observe(ev("e1", "s", "a", "2020-01-01T00:00:00Z"));
    let after_first = c.wal_len() as u64;
    c.observe(ev("e2", "s", "b", "2021-01-01T00:00:00Z"));
    let tail = c.wal_since(after_first);
    assert_eq!(tail.len(), c.wal_len() - after_first as usize);
    assert!(tail.iter().all(|e| e.seq > after_first));
}

#[test]
fn observe_then_recall_returns_event() {
    let mut c = Core::default();
    c.observe(ev(
        "e1",
        "neutrino",
        "neutrinos have mass",
        "2020-01-01T00:00:00Z",
    ));
    let r = c.recall(&q("neutrino"));
    assert!(r.used_ids.contains(&"e1".to_string()));
    assert!(!r.context_pack_hash.is_empty());
}

#[test]
fn rebuild_preserves_state_hash() {
    let mut c = Core::default();
    c.observe(ev(
        "e1",
        "neutrino",
        "mass is small",
        "2020-01-01T00:00:00Z",
    ));
    c.observe(ev(
        "e2",
        "muon",
        "heavier than electron",
        "2020-01-01T00:00:00Z",
    ));
    c.recall(&q("neutrino"));
    c.recall(&q("muon"));
    let h1 = c.export_state_hash();
    c.rebuild();
    let h2 = c.export_state_hash();
    assert_eq!(h1, h2);
}

#[test]
fn historical_recall_does_not_mutate() {
    let mut c = Core::default();
    c.observe(ev("prior", "subj", "prior fact", "2020-01-01T00:00:00Z"));
    c.observe(ev(
        "current",
        "subj",
        "current fact",
        "2025-01-01T00:00:00Z",
    ));
    let before = c.export_state_hash();
    c.recall_as_of(&q("subj"), "2022-06-01T00:00:00Z");
    c.recall_at(&q("subj"), "2022-06-01T00:00:00Z");
    let after = c.export_state_hash();
    assert_eq!(before, after);
}

#[test]
fn vault_event_is_redacted() {
    let mut c = Core::default();
    let mut e = ev("v1", "API key", "API_KEY=abc", "2026-01-01T00:00:00Z");
    e.privacy_class = PrivacyClass::Vault;
    e.kind = "VaultCanary".to_string();
    c.observe(e);
    let r = c.recall(&q("API key"));
    assert!(r.answer.contains("[REDACTED"));
    assert!(r.warnings.contains(&Warning::Redacted));
}

#[test]
fn canary_body_is_redacted() {
    let mut c = Core::default();
    let canary = format!("{}{}{}{}{}", "s", "k-memory-", "b", "ench-", "CANARY-7f3a");
    let body = format!("token={canary}");
    c.observe(ev("c1", "secret", &body, "2026-01-01T00:00:00Z"));
    let r = c.recall(&q("secret"));
    assert!(!r.answer.contains(&canary));
    assert!(r.warnings.contains(&Warning::Redacted));
}

#[test]
fn supersession_partner_surfaces_skeptic() {
    let mut c = Core::default();
    let mut earlier = ev("a", "subj", "prior body", "2020-01-01T00:00:00Z");
    earlier.valid_from = Some("2020-01-01T00:00:00Z".to_string());
    let mut newer = ev("b", "subj", "current body", "2025-01-01T00:00:00Z");
    newer.valid_from = Some("2024-01-01T00:00:00Z".to_string());
    c.observe(earlier);
    c.observe(newer);
    let r = c.recall(&q("subj"));
    assert!(r.warnings.contains(&Warning::SkeptikSurfaced));
}

#[test]
fn feedback_moves_hebb_and_utility() {
    let mut c = Core::default();
    c.observe(ev("a", "x", "y", "2020-01-01T00:00:00Z"));
    c.observe(ev("b", "x2", "y2", "2020-01-01T00:00:00Z"));
    c.recall(&q("x"));
    c.feedback(&FeedbackSignal {
        outcome: Outcome::TaskSuccess,
        used: vec!["a".to_string(), "b".to_string()],
    });
    assert!(c.hebb.weight(0, 1) > 0.0);
}

#[test]
fn unsafe_skill_in_procedure_is_refused() {
    let mut c = Core::default();
    let mut e = ev("s1", "tool_x", "UNSAFE side-effect", "2026-01-01T00:00:00Z");
    e.kind = "Skill".to_string();
    e.tags.push("unsafe".to_string());
    c.observe(e);
    let r = c.recall(&RecallQuery {
        text: "tool_x".to_string(),
        mentions: Vec::new(),
        intent: Intent::Procedure,
        token_budget: 4096,
    });
    assert!(r.warnings.contains(&Warning::UnsafeToolRefused));
    assert!(r.answer.contains("refused"));
}

#[test]
fn recall_touch_promotes_strength() {
    let mut c = Core::default();
    c.observe(ev("a", "neutrino", "has mass", "2020-01-01T00:00:00Z"));
    let before = c.cells[0].strength;
    for _ in 0..5 {
        c.recall(&q("neutrino"));
    }
    let after = c.cells[0].strength;
    assert!(
        after > before,
        "strength must increase after repeated recalls (before={before}, after={after})"
    );
}
