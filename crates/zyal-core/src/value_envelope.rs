//! Structural taint + the normalized wrapper for every external value.
//!
//! Law #6 (taint is structural, not advisory): every edge carries taint labels;
//! `web_content` / `prod_data` / `secret` may inform reasoning but cannot arm
//! host actions, write durable memory, or exec shell unless a declared sanitizer
//! transformed the value (provenance retained). This module is the single home
//! of that vocabulary, shared by the compiler (edge labels), the runner
//! (`decide_arm` enforcement), the source/research/artifact subsystems
//! (envelopes), and the cockpit (display).
//!
//! Kept pure (serde-only, no I/O, no hashing) so it can live in the
//! dependency-light `zyal-core` crate. `content_hash` computation lives in the
//! runner crates that already depend on `sha2`; `ProvenanceRef::content_hash`
//! here is just the resulting string.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Provenance category of a value. Categorical (what kind of source), distinct
/// from [`TaintRank`] (how dangerous). A value can carry several labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaintLabel {
    /// First-party, operator-authored, or otherwise trusted input.
    Trusted,
    /// Fetched from the open web — treat as potentially adversarial.
    WebContent,
    /// Production / customer data — sensitive, must not leak.
    ProdData,
    /// Free-form model output — unverified, may contain injected instructions.
    ModelOutput,
    /// Explicitly untrusted (user-uploaded, third-party connector, …).
    Untrusted,
    /// A secret (credential/key/token). Never leaves to a remote model.
    Secret,
    /// A declared sanitizer transformed the value; downgrades arming gates for
    /// non-secret taint while retaining provenance.
    Sanitized,
}

/// Derived severity used for sorting/display. The real arming decision is
/// [`TaintSet::can_arm`]; this is a convenience summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaintRank {
    Clean,
    Medium,
    Untrusted,
    UntrustedForArming,
    Hostile,
}

/// The class of effect a value is about to be used for. The gate differs per
/// class (e.g. a secret may never reach a remote model, but sanitized web
/// content may arm a host action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    /// Arm a host-side action (write a file, hit a webhook, run a connector).
    ArmHostAction,
    /// Write into durable shared memory.
    WriteDurableMemory,
    /// Execute a shell command.
    ExecShell,
    /// Send the value to a remote model provider.
    RemoteModelCall,
}

/// A set of taint labels plus the provenance ids it was derived from. Ordered
/// (BTreeSet) so serialization is deterministic (law #2).
///
/// CAUTION: `TaintSet::default()` is EMPTY — it represents UNKNOWN taint, not a
/// safe value. An empty set passes `can_arm` (no blocking labels), so never use
/// `default()` as a semantic "safe" default for data of unknown origin: use
/// `clean()` for genuinely-trusted data, or `from_labels([Untrusted])` to be
/// fail-closed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaintSet {
    #[serde(default)]
    pub labels: BTreeSet<TaintLabel>,
    /// Provenance ids (envelope/source ids) this taint flowed from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<String>,
}

impl TaintSet {
    pub fn clean() -> Self {
        let mut labels = BTreeSet::new();
        labels.insert(TaintLabel::Trusted);
        Self {
            labels,
            derived_from: Vec::new(),
        }
    }

    pub fn from_labels<I: IntoIterator<Item = TaintLabel>>(labels: I) -> Self {
        Self {
            labels: labels.into_iter().collect(),
            derived_from: Vec::new(),
        }
    }

    pub fn has(&self, label: TaintLabel) -> bool {
        self.labels.contains(&label)
    }

    /// Combine the taint of two inputs flowing into the same node. Union of
    /// labels + provenance. Commutative, associative, deterministic.
    pub fn join(&self, other: &TaintSet) -> TaintSet {
        let mut labels = self.labels.clone();
        labels.extend(other.labels.iter().copied());
        let mut derived_from = self.derived_from.clone();
        derived_from.extend(other.derived_from.iter().cloned());
        derived_from.sort();
        derived_from.dedup();
        TaintSet {
            labels,
            derived_from,
        }
    }

    /// Derived severity (display/sort only — use [`can_arm`] for decisions).
    pub fn rank(&self) -> TaintRank {
        if self.has(TaintLabel::Secret) {
            return TaintRank::Hostile;
        }
        let sanitized = self.has(TaintLabel::Sanitized);
        if self.has(TaintLabel::Untrusted) && !sanitized {
            return TaintRank::UntrustedForArming;
        }
        let active = self.has(TaintLabel::WebContent)
            || self.has(TaintLabel::ProdData)
            || self.has(TaintLabel::ModelOutput);
        if active && !sanitized {
            return TaintRank::Untrusted;
        }
        if active || sanitized || self.has(TaintLabel::Untrusted) {
            return TaintRank::Medium;
        }
        TaintRank::Clean
    }

    /// The structural arming gate. Returns `false` (fail-closed) when the taint
    /// would unsafely arm the given action class.
    pub fn can_arm(&self, action: ActionClass) -> bool {
        let sanitized = self.has(TaintLabel::Sanitized);
        match action {
            // A secret must never leave to a remote model; no sanitizer fixes that.
            ActionClass::RemoteModelCall => !self.has(TaintLabel::Secret),
            ActionClass::ExecShell
            | ActionClass::ArmHostAction
            | ActionClass::WriteDurableMemory => {
                if self.has(TaintLabel::Secret) {
                    return false;
                }
                let tainted = self.has(TaintLabel::WebContent)
                    || self.has(TaintLabel::ProdData)
                    || self.has(TaintLabel::Untrusted)
                    || self.has(TaintLabel::ModelOutput);
                !tainted || sanitized
            }
        }
    }
}

/// Apply an edge's taint transfer function. A declared `sanitizer` adds the
/// `Sanitized` label (downgrading arming gates for non-secret taint) while
/// retaining the provenance trail. Deterministic and order-independent.
pub fn transfer_taint(src: &TaintSet, sanitizer: Option<&str>) -> TaintSet {
    let mut out = src.clone();
    if let Some(name) = sanitizer {
        out.labels.insert(TaintLabel::Sanitized);
        let marker = format!("sanitizer:{name}");
        if !out.derived_from.contains(&marker) {
            out.derived_from.push(marker);
            out.derived_from.sort();
        }
    }
    out
}

/// Where a value came from (kept secret-free: `uri_redacted`, not raw URLs with
/// embedded credentials). `content_hash` is computed by the runner crates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRef {
    pub id: String,
    pub kind: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri_redacted: Option<String>,
    /// `sha256:<hex>` of the source bytes (runner-computed).
    #[serde(default)]
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub receipt_path: String,
}

/// One normalized wrapper for every external value entering a run: a feed tick,
/// a fetched page, a tool output, a shared-memory write. Carries provenance +
/// taint so downstream gates are structural, not advisory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueEnvelope {
    pub id: String,
    pub run_id: String,
    pub source_node_id: String,
    pub seq: u64,
    pub value: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_value: Option<f64>,
    pub value_schema_id: String,
    pub observed_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ts_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_ms: Option<u64>,
    pub provenance: ProvenanceRef,
    pub taint: TaintSet,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_never_reaches_a_remote_model() {
        let secret = TaintSet::from_labels([TaintLabel::Secret]);
        assert!(!secret.can_arm(ActionClass::RemoteModelCall));
        // even after a sanitizer, a secret cannot leave to a remote model
        let sanitized = transfer_taint(&secret, Some("redactor"));
        assert!(!sanitized.can_arm(ActionClass::RemoteModelCall));
    }

    #[test]
    fn untrusted_cannot_exec_shell_until_sanitized() {
        let web = TaintSet::from_labels([TaintLabel::WebContent]);
        assert!(!web.can_arm(ActionClass::ExecShell));
        assert!(!web.can_arm(ActionClass::ArmHostAction));
        let sanitized = transfer_taint(&web, Some("html_strip"));
        assert!(sanitized.can_arm(ActionClass::ArmHostAction));
        assert!(sanitized.can_arm(ActionClass::ExecShell));
        assert!(sanitized.has(TaintLabel::Sanitized));
        // provenance retained
        assert!(sanitized
            .derived_from
            .iter()
            .any(|d| d.contains("html_strip")));
    }

    #[test]
    fn clean_values_arm_freely() {
        let clean = TaintSet::clean();
        for a in [
            ActionClass::ArmHostAction,
            ActionClass::WriteDurableMemory,
            ActionClass::ExecShell,
            ActionClass::RemoteModelCall,
        ] {
            assert!(clean.can_arm(a));
        }
        assert_eq!(clean.rank(), TaintRank::Clean);
    }

    #[test]
    fn join_is_commutative_associative_deterministic() {
        let a = TaintSet::from_labels([TaintLabel::WebContent]);
        let b = TaintSet::from_labels([TaintLabel::ProdData]);
        let c = TaintSet::from_labels([TaintLabel::ModelOutput]);
        assert_eq!(a.join(&b), b.join(&a));
        assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));
        // deterministic serialization (BTreeSet order)
        let j = a.join(&b).join(&c);
        assert_eq!(
            serde_json::to_string(&j).unwrap(),
            serde_json::to_string(&a.join(&b).join(&c)).unwrap()
        );
    }

    #[test]
    fn rank_ordering_is_total() {
        assert!(TaintRank::Clean < TaintRank::Medium);
        assert!(TaintRank::Medium < TaintRank::Untrusted);
        assert!(TaintRank::Untrusted < TaintRank::UntrustedForArming);
        assert!(TaintRank::UntrustedForArming < TaintRank::Hostile);
    }
}
