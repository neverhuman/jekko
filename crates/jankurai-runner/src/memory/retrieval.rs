//! Phase E2 retrieval-augmented memory lookup.
//!
//! Given a query, embed it once, scan promoted capsules (project_only or
//! global), score by cosine similarity against per-capsule embeddings, and
//! return the top-k along with their similarity scores. The orchestrator
//! turns the top-k into a `<prior_lessons>` block via
//! [`format_lessons_prompt_block`].

use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use jekko_store::daemon::reasoning::{cosine_similarity, decode_embedding};
use jekko_store::daemon::{list_promoted_capsules, MemoryCapsuleRow};
use jekko_store::db::Db;

use super::embedder::Embedder;

/// Knobs for [`retrieve_for_run`]. The defaults are tuned for the Phase E2
/// reasoning orchestrator: 8 capsules ≈ ~2k tokens of injected lessons.
#[derive(Clone, Debug)]
pub struct RetrievalConfig {
    /// Cap on the number of capsules returned. Higher = more recall, more
    /// prompt tokens consumed.
    pub top_k: usize,
    /// Restrict the candidate pool to a single scope (e.g. project id).
    /// `None` lets all promoted capsules through.
    pub scope: Option<String>,
    /// Drop capsules whose `time_updated` is older than this many days.
    /// `None` disables age filtering — useful when the store is fresh.
    pub max_age_days: Option<u32>,
    /// Soft cap on the byte/token budget for the rendered prompt block.
    /// `format_lessons_prompt_block` uses this to truncate.
    pub max_tokens_for_injection: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            top_k: 8,
            scope: None,
            max_age_days: None,
            max_tokens_for_injection: 2000,
        }
    }
}

/// Embed `query`, scan the promoted capsule pool, and return the top-k
/// `(capsule, similarity)` pairs sorted by similarity descending.
///
/// Capsules from `current_run_id` are demoted (similarity * 0.5) rather
/// than dropped, so a run can still surface a same-run lesson when nothing
/// better exists but never poisons its own loop by promoting itself first.
/// Capsules with no embedding or a corrupted blob are skipped silently.
pub async fn retrieve_for_run(
    db: &Db,
    embedder: &dyn Embedder,
    query: &str,
    current_run_id: &str,
    config: &RetrievalConfig,
) -> Result<Vec<(MemoryCapsuleRow, f32)>> {
    let query_vec = embedder.embed(query).await?;
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let candidates = list_promoted_capsules(
        db.connection(),
        config.scope.as_deref(),
        None,
        config.max_age_days,
        now_secs,
    )?;

    let mut scored: Vec<(MemoryCapsuleRow, f32)> = Vec::new();
    for capsule in candidates {
        let Some(blob) = capsule.embedding.as_ref() else {
            continue;
        };
        let Some(capsule_vec) = decode_embedding(blob) else {
            continue;
        };
        let Some(mut sim) = cosine_similarity(&query_vec, &capsule_vec) else {
            continue;
        };
        if capsule.run_id == current_run_id {
            sim *= 0.5;
        }
        scored.push((capsule, sim));
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    scored.truncate(config.top_k);
    Ok(scored)
}

/// Render the top-k capsules as a `<prior_lessons>` XML-ish block ready to
/// concatenate into a system prompt. Truncates so the final block fits the
/// `max_tokens` budget (estimated 4 chars per token).
pub fn format_lessons_prompt_block(top_k: &[(MemoryCapsuleRow, f32)], max_tokens: usize) -> String {
    let max_chars = max_tokens.saturating_mul(4);
    let mut body = String::new();
    for (capsule, score) in top_k {
        let claim = pick_claim(capsule);
        let line = format!("- [score={:.2}] {claim}\n", score);
        if body.len() + line.len() > max_chars {
            break;
        }
        body.push_str(&line);
    }
    if body.is_empty() {
        return String::new();
    }
    format!("<prior_lessons>\n{body}</prior_lessons>")
}

fn pick_claim(capsule: &MemoryCapsuleRow) -> String {
    if !capsule.claim_text.trim().is_empty() {
        return capsule.claim_text.trim().to_string();
    }
    if !capsule.summary.trim().is_empty() {
        return capsule.summary.trim().to_string();
    }
    capsule.id.clone()
}
