use anyhow::Result;
use jekko_store::daemon::{self, ModelOutcomeRow};
use jekko_store::db::Db;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::path::Path;

use crate::model_client::ModelCallReceipt;

use super::helpers::now_ms;

#[derive(Debug)]
enum ReceiptPayloadState {
    Present(Value),
    MissingPayload,
}

impl ReceiptPayloadState {
    fn from_optional(payload: Option<Value>) -> Self {
        match payload {
            Some(payload) => Self::Present(payload),
            None => Self::MissingPayload,
        }
    }

    fn state_name(&self) -> &'static str {
        match self {
            Self::Present(_) => "present",
            Self::MissingPayload => "missing_payload",
        }
    }

    fn is_missing(&self) -> bool {
        matches!(self, Self::MissingPayload)
    }

    fn field(&self, name: &str) -> Value {
        match self {
            Self::Present(payload) => match payload.get(name) {
                Some(value) => value.clone(),
                None => Value::Null,
            },
            Self::MissingPayload => Value::Null,
        }
    }
}

/// Persist a model call receipt in `daemon_model_outcome`.
pub fn persist_model_receipt(db: &Db, run_id: &str, receipt: &ModelCallReceipt) -> Result<()> {
    daemon::upsert_model_outcome(
        db.connection(),
        &ModelOutcomeRow {
            id: receipt.id.clone(),
            run_id: run_id.to_string(),
            task_id: receipt.task_id.clone(),
            model_id: receipt.model.clone(),
            role: receipt.kind.clone(),
            cost_usd: receipt.cost_usd,
            latency_ms: Some(receipt.latency_ms as i64),
            status: if receipt.success {
                "success".to_string()
            } else {
                "failure".to_string()
            },
            reviewer_score: None,
            winner: receipt.success,
            payload_json: Some(json!({
                "provider": receipt.provider,
                "response_sha256": receipt.response.as_ref().map(|response| {
                    let mut hasher = Sha1::new();
                    hasher.update(response.as_bytes());
                    format!("{:x}", hasher.finalize())
                }),
                "response_bytes": receipt.response.as_ref().map(|response| response.len()),
                "error": receipt.error,
                "budget_used": receipt.budget_used,
                "budget_remaining": receipt.budget_remaining,
                "route": receipt.route,
                "credential_policy": receipt.credential_policy,
                "selected_credential_user_id": receipt.selected_credential_user_id,
                "credential_user_id": receipt.credential_user_id,
                "retry_count": receipt.retry_count,
            })),
            time_created: now_ms(),
            time_updated: now_ms(),
        },
    )?;
    daemon::record_model_reliability_outcome(
        db.connection(),
        &receipt.model,
        &receipt.kind,
        &receipt.kind,
        receipt.success,
        receipt.success,
        receipt.latency_ms as i64,
        receipt.cost_usd.unwrap_or(0.0),
        now_ms(),
    )?;
    Ok(())
}

/// Export sanitized model receipts for independent run-directory audits.
///
/// The raw assistant text is never written here; only response hashes, byte
/// counts, credential provenance, budget counters, and routing metadata are
/// exported.
pub fn export_model_receipts_jsonl(db: &Db, run_id: &str, path: &Path) -> Result<()> {
    let rows = daemon::list_model_outcomes_for_run(db.connection(), run_id)?;
    let receipts = rows
        .into_iter()
        .map(|row| {
            let payload = ReceiptPayloadState::from_optional(row.payload_json);
            json!({
                "schema_version": "zyal.model_receipt.v1",
                "id": row.id,
                "run_id": row.run_id,
                "kind": row.role,
                "provider": payload.field("provider"),
                "model": row.model_id,
                "status": row.status,
                "success": row.winner,
                "latency_ms": row.latency_ms,
                "cost_usd": row.cost_usd,
                "payload_state": payload.state_name(),
                "payload_missing": payload.is_missing(),
                "response_sha256": payload.field("response_sha256"),
                "response_bytes": payload.field("response_bytes"),
                "error": payload.field("error"),
                "budget_used": payload.field("budget_used"),
                "budget_remaining": payload.field("budget_remaining"),
                "route": payload.field("route"),
                "credential_policy": payload.field("credential_policy"),
                "selected_credential_user_id": payload.field("selected_credential_user_id"),
                "credential_user_id": payload.field("credential_user_id"),
                "retry_count": payload.field("retry_count"),
            })
        })
        .collect::<Vec<_>>();
    crate::hero_judge_eval::write_jsonl(path, &receipts)
}
