//! Helpers for observing chat turns without exposing cogcore event plumbing to
//! server or CLI callers.

use chrono::{TimeZone, Utc};
use cogcore::core::{ClaimModality, PrivacyClass, StoredEvent};

/// One user/assistant/tool turn to observe into session memory.
#[derive(Debug, Clone)]
pub struct TurnObservation {
    /// Stable event id for the turn.
    pub id: String,
    /// Session or scope id.
    pub session_id: String,
    /// Role tag, usually `user`, `assistant`, or `tool`.
    pub role: String,
    /// Plain text body to make recall searchable.
    pub body: String,
    /// Event time in Unix milliseconds.
    pub time_ms: i64,
}

impl TurnObservation {
    /// Build a turn observation.
    pub fn new(
        id: impl Into<String>,
        session_id: impl Into<String>,
        role: impl Into<String>,
        body: impl Into<String>,
        time_ms: i64,
    ) -> Self {
        Self {
            id: id.into(),
            session_id: session_id.into(),
            role: role.into(),
            body: body.into(),
            time_ms,
        }
    }

    /// Convert into the `cogcore` event shape consumed by [`crate::MemoryService`].
    pub fn into_stored_event(self) -> StoredEvent {
        let role = self.role;
        let time = ms_to_iso(self.time_ms);
        let claim_modality = Some(if role == "assistant" {
            ClaimModality::InferredByAgent
        } else {
            ClaimModality::Observed
        });
        StoredEvent {
            id: self.id,
            kind: "turn_appended".to_string(),
            subject: self.session_id,
            body: self.body,
            tx_time: time.clone(),
            valid_from: Some(time),
            valid_to: None,
            privacy_class: PrivacyClass::Internal,
            claim_modality,
            tags: vec!["turn_appended".to_string(), format!("role:{role}")],
            sources: Vec::new(),
            supersedes: Vec::new(),
            contradicts: Vec::new(),
        }
    }
}

fn ms_to_iso(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_turn_is_inferred_by_agent() {
        let ev = TurnObservation::new("m1", "s1", "assistant", "answer", 1_700_000_000_000)
            .into_stored_event();
        assert_eq!(ev.kind, "turn_appended");
        assert_eq!(ev.subject, "s1");
        assert_eq!(ev.body, "answer");
        assert_eq!(ev.claim_modality, Some(ClaimModality::InferredByAgent));
        assert!(ev.tags.contains(&"role:assistant".to_string()));
        assert!(ev.tx_time.starts_with("2023-11-14"));
    }

    #[test]
    fn user_turn_is_observed() {
        let ev = TurnObservation::new("m1", "s1", "user", "question", 0).into_stored_event();
        assert_eq!(ev.claim_modality, Some(ClaimModality::Observed));
        assert_eq!(ev.valid_from.as_deref(), Some("1970-01-01T00:00:00.000Z"));
    }
}
