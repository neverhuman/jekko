//! Unit tests for the Phase E2 memory module.

use jekko_store::daemon::reasoning::encode_embedding;
use jekko_store::daemon::{upsert_memory_capsule, MemoryCapsuleRow};
use jekko_store::db::Db;

use super::embedder::{Embedder, FakeEmbedder};
use super::retrieval::{format_lessons_prompt_block, retrieve_for_run, RetrievalConfig};

/// Tests focus on the retrieval/ranking logic, not on FK chains into
/// daemon_run + daemon_reasoning_artifact, so we disable FK enforcement
/// for the in-memory test DB the same way `daemon_port_roundtrip` does.
fn open_test_db() -> Db {
    let db = Db::open_in_memory().unwrap();
    db.connection()
        .execute_batch("PRAGMA foreign_keys = OFF")
        .unwrap();
    db
}

fn capsule(
    id: &str,
    run_id: &str,
    promotion: &str,
    claim: &str,
    embedding: Option<Vec<f32>>,
) -> MemoryCapsuleRow {
    MemoryCapsuleRow {
        id: id.to_string(),
        run_id: run_id.to_string(),
        artifact_id: format!("artifact-{id}"),
        scope: "project-default".to_string(),
        status: "verified".to_string(),
        summary: format!("summary for {id}"),
        evidence_level: "strong".to_string(),
        confidence: 0.9,
        payload_json: None,
        content_hash: format!("hash-{id}"),
        time_created: 1000,
        time_updated: 2000,
        memory_kind: "semantic".to_string(),
        promotion_status: promotion.to_string(),
        claim_text: claim.to_string(),
        approved_by_role: Some("verifier".to_string()),
        embedding: embedding.map(|v| encode_embedding(&v)),
    }
}

#[tokio::test]
async fn fake_embedder_returns_configured_vector() {
    let embedder = FakeEmbedder::new(vec![0.1, 0.2, 0.3]);
    let out = embedder.embed("anything").await.unwrap();
    assert_eq!(out, vec![0.1, 0.2, 0.3]);
    // Same input again produces the same vec (determinism contract).
    let out2 = embedder.embed("different").await.unwrap();
    assert_eq!(out, out2);
}

#[tokio::test]
async fn retrieve_for_run_filters_promoted_only() {
    let db = open_test_db();
    let conn = db.connection();
    let vec_a = vec![1.0_f32, 0.0, 0.0];
    upsert_memory_capsule(
        conn,
        &capsule(
            "cap-scratch",
            "run-other",
            "scratch",
            "scratch lesson",
            Some(vec_a.clone()),
        ),
    )
    .unwrap();
    upsert_memory_capsule(
        conn,
        &capsule(
            "cap-project",
            "run-other",
            "project_only",
            "project lesson",
            Some(vec_a.clone()),
        ),
    )
    .unwrap();

    let embedder = FakeEmbedder::new(vec_a);
    let config = RetrievalConfig::default();
    let results = retrieve_for_run(&db, &embedder, "query", "run-current", &config)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0.id, "cap-project");
}

#[tokio::test]
async fn retrieve_for_run_demotes_same_run_capsules() {
    let db = open_test_db();
    let conn = db.connection();
    let vec_a = vec![1.0_f32, 0.0, 0.0];
    upsert_memory_capsule(
        conn,
        &capsule(
            "cap-same-run",
            "run-current",
            "project_only",
            "same run lesson",
            Some(vec_a.clone()),
        ),
    )
    .unwrap();
    upsert_memory_capsule(
        conn,
        &capsule(
            "cap-other-run",
            "run-other",
            "project_only",
            "other run lesson",
            Some(vec_a.clone()),
        ),
    )
    .unwrap();

    let embedder = FakeEmbedder::new(vec_a);
    let config = RetrievalConfig::default();
    let results = retrieve_for_run(&db, &embedder, "query", "run-current", &config)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    // The other-run capsule should rank higher (no demotion) than the
    // same-run capsule (similarity * 0.5).
    assert_eq!(results[0].0.id, "cap-other-run");
    assert_eq!(results[1].0.id, "cap-same-run");
    assert!(results[0].1 > results[1].1);
    assert!((results[0].1 - 2.0 * results[1].1).abs() < 1e-5);
}

#[tokio::test]
async fn retrieve_for_run_respects_top_k() {
    let db = open_test_db();
    let conn = db.connection();
    let vec_a = vec![1.0_f32, 0.0, 0.0];
    for i in 0..5 {
        upsert_memory_capsule(
            conn,
            &capsule(
                &format!("cap-{i}"),
                "run-other",
                "project_only",
                &format!("lesson {i}"),
                Some(vec_a.clone()),
            ),
        )
        .unwrap();
    }
    let embedder = FakeEmbedder::new(vec_a);
    let config = RetrievalConfig {
        top_k: 2,
        ..RetrievalConfig::default()
    };
    let results = retrieve_for_run(&db, &embedder, "query", "run-current", &config)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn format_lessons_prompt_block_truncates_at_token_budget() {
    // ~4 chars per token => 20 tokens ≈ 80 chars budget. The first line
    // ("- [score=0.90] short claim a\n" = 30 chars) fits; the second
    // long line would push past, so it must be dropped.
    let cap_a = capsule(
        "cap-a",
        "run-a",
        "project_only",
        "short claim a",
        Some(vec![1.0]),
    );
    let cap_b = capsule(
        "cap-b",
        "run-b",
        "project_only",
        "another claim that is long enough to push the body past the eighty-char budget",
        Some(vec![1.0]),
    );
    let block = format_lessons_prompt_block(&[(cap_a.clone(), 0.9), (cap_b.clone(), 0.8)], 20);
    assert!(block.starts_with("<prior_lessons>"));
    assert!(block.ends_with("</prior_lessons>"));
    assert!(block.contains(&cap_a.claim_text));
    // Budget would overflow when adding cap_b; truncation must drop it.
    assert!(!block.contains(&cap_b.claim_text));
}

#[test]
fn format_lessons_prompt_block_renders_score_and_claim() {
    let cap = capsule("c1", "r1", "project_only", "short claim", Some(vec![1.0]));
    let block = format_lessons_prompt_block(&[(cap, 0.875)], 1000);
    assert!(block.contains("score=0.88") || block.contains("score=0.87"));
    assert!(block.contains("short claim"));
}
