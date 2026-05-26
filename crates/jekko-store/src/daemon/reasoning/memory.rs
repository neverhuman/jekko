use rusqlite::{params, Connection};

use crate::error::StoreResult;

use super::rows::MemoryCapsuleRow;
use crate::daemon::support::{collect_rows, parse_opt_json, serialize_opt};

/// Insert or replace a memory capsule.
pub fn upsert_memory_capsule(conn: &Connection, row: &MemoryCapsuleRow) -> StoreResult<()> {
    let payload = serialize_opt(&row.payload_json)?;
    conn.execute(
        "INSERT INTO daemon_memory_capsule (
            id, run_id, artifact_id, scope, status, summary, evidence_level,
            confidence, payload_json, content_hash, time_created, time_updated,
            memory_kind, promotion_status, claim_text, approved_by_role, embedding
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
        ON CONFLICT(id) DO UPDATE SET
            scope = excluded.scope,
            status = excluded.status,
            summary = excluded.summary,
            evidence_level = excluded.evidence_level,
            confidence = excluded.confidence,
            payload_json = excluded.payload_json,
            content_hash = excluded.content_hash,
            time_updated = excluded.time_updated,
            memory_kind = excluded.memory_kind,
            promotion_status = excluded.promotion_status,
            claim_text = excluded.claim_text,
            approved_by_role = excluded.approved_by_role,
            embedding = excluded.embedding",
        params![
            row.id,
            row.run_id,
            row.artifact_id,
            row.scope,
            row.status,
            row.summary,
            row.evidence_level,
            row.confidence,
            payload,
            row.content_hash,
            row.time_created,
            row.time_updated,
            row.memory_kind,
            row.promotion_status,
            row.claim_text,
            row.approved_by_role,
            row.embedding,
        ],
    )?;
    Ok(())
}

/// List memory capsules for a run.
pub fn list_memory_capsules_for_run(
    conn: &Connection,
    run_id: &str,
) -> StoreResult<Vec<MemoryCapsuleRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, artifact_id, scope, status, summary, evidence_level,
                confidence, payload_json, content_hash, time_created, time_updated,
                memory_kind, promotion_status, claim_text, approved_by_role, embedding
         FROM daemon_memory_capsule WHERE run_id = ?1 ORDER BY time_created ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![run_id], memory_capsule_from_row)?;
    collect_rows(rows)
}

/// List capsules whose promotion status is `project_only` or `global` —
/// i.e., visible cross-run. Optional filters narrow by scope, by memory
/// kind, and by max age in days. Pass `None` for any filter to disable it.
///
/// Phase E2's `find_similar_capsules` will pre-filter via this helper
/// before running cosine similarity over the candidate set.
pub fn list_promoted_capsules(
    conn: &Connection,
    scope: Option<&str>,
    kind: Option<&str>,
    max_age_days: Option<u32>,
    now_secs: i64,
) -> StoreResult<Vec<MemoryCapsuleRow>> {
    let mut sql = String::from(
        "SELECT id, run_id, artifact_id, scope, status, summary, evidence_level,
                confidence, payload_json, content_hash, time_created, time_updated,
                memory_kind, promotion_status, claim_text, approved_by_role, embedding
         FROM daemon_memory_capsule
         WHERE promotion_status IN ('project_only', 'global')",
    );
    let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(scope) = scope {
        sql.push_str(" AND scope = ?");
        params_vec.push(scope.to_string().into());
    }
    if let Some(kind) = kind {
        sql.push_str(" AND memory_kind = ?");
        params_vec.push(kind.to_string().into());
    }
    if let Some(days) = max_age_days {
        sql.push_str(" AND time_updated >= ?");
        let cutoff = now_secs - (days as i64) * 86_400;
        params_vec.push(cutoff.into());
    }
    sql.push_str(" ORDER BY time_updated DESC, id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec
        .iter()
        .map(|v| v as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(param_refs.as_slice(), memory_capsule_from_row)?;
    collect_rows(rows)
}

fn memory_capsule_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryCapsuleRow> {
    let payload_text: Option<String> = row.get(8)?;
    Ok(MemoryCapsuleRow {
        id: row.get(0)?,
        run_id: row.get(1)?,
        artifact_id: row.get(2)?,
        scope: row.get(3)?,
        status: row.get(4)?,
        summary: row.get(5)?,
        evidence_level: row.get(6)?,
        confidence: row.get(7)?,
        payload_json: parse_opt_json(8, payload_text)?,
        content_hash: row.get(9)?,
        time_created: row.get(10)?,
        time_updated: row.get(11)?,
        memory_kind: row.get(12)?,
        promotion_status: row.get(13)?,
        claim_text: row.get(14)?,
        approved_by_role: row.get(15)?,
        embedding: row.get(16)?,
    })
}

/// Encode an f32 vector to a little-endian byte blob suitable for the
/// `embedding` column. Reverse with [`decode_embedding`].
pub fn encode_embedding(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Decode a little-endian f32 byte blob (as stored in the `embedding`
/// column) back to a vector. Returns `None` when the byte length is not a
/// multiple of 4 (corrupted row).
pub fn decode_embedding(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().ok()?;
        out.push(f32::from_le_bytes(arr));
    }
    Some(out)
}

/// Cosine similarity between two equal-length vectors. Returns `None` when
/// the vectors differ in length or either has zero norm. Used by Phase E2's
/// `find_similar_capsules` brute-force ranker.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return None;
    }
    Some(dot / (norm_a.sqrt() * norm_b.sqrt()))
}

#[cfg(test)]
mod embedding_tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let v = vec![0.0_f32, 1.0, -2.5, std::f32::consts::PI];
        let bytes = encode_embedding(&v);
        assert_eq!(bytes.len(), v.len() * 4);
        let back = decode_embedding(&bytes).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn decode_rejects_misaligned_bytes() {
        assert_eq!(decode_embedding(&[1, 2, 3]), None);
    }

    #[test]
    fn cosine_of_identical_vectors_is_one() {
        let v = vec![1.0_f32, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v).unwrap();
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_of_orthogonal_vectors_is_zero() {
        let sim = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).unwrap();
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn cosine_of_mismatched_length_is_none() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0, 2.0, 3.0]), None);
    }

    #[test]
    fn cosine_of_zero_vector_is_none() {
        assert_eq!(cosine_similarity(&[0.0, 0.0, 0.0], &[1.0, 2.0, 3.0]), None);
    }
}
