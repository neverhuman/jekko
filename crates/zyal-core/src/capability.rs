//! Capability vocabulary for the trust pipeline (F5).
//!
//! A capability is a named, gated power a node/tool may request. The runner's
//! fail-closed authorize pipeline (M6) grants a lease only after every guard in
//! the chain passes. Two capabilities are deny-always: a node can never export
//! a credential or read a raw secret value — those are not grantable at any
//! disposition.
//!
//! Pure serde + a static disposition table; enforcement lives in the runner.

use serde::{Deserialize, Serialize};

/// A gated power. Serialized as its dotted id (`shell.exec`) so it reads the
/// same in YAML, the registry, events, and the cockpit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    #[serde(rename = "shell.exec")]
    ShellExec,
    #[serde(rename = "code.exec")]
    CodeExec,
    #[serde(rename = "network.fetch")]
    NetworkFetch,
    /// Deny-always: a node may never export a credential.
    #[serde(rename = "credential.export")]
    CredentialExport,
    /// Deny-always: a node may never read a raw secret value.
    #[serde(rename = "env.read_secret")]
    EnvReadSecret,
    #[serde(rename = "vector.read")]
    VectorRead,
    #[serde(rename = "vector.write")]
    VectorWrite,
    #[serde(rename = "sql.read")]
    SqlRead,
    #[serde(rename = "sql.write")]
    SqlWrite,
    #[serde(rename = "workflow.call")]
    WorkflowCall,
    #[serde(rename = "model.call")]
    ModelCall,
    #[serde(rename = "scheduler.write")]
    SchedulerWrite,
    #[serde(rename = "webhook.listen")]
    WebhookListen,
}

impl Capability {
    /// The dotted id, e.g. `"shell.exec"`.
    pub fn id(&self) -> &'static str {
        match self {
            Capability::ShellExec => "shell.exec",
            Capability::CodeExec => "code.exec",
            Capability::NetworkFetch => "network.fetch",
            Capability::CredentialExport => "credential.export",
            Capability::EnvReadSecret => "env.read_secret",
            Capability::VectorRead => "vector.read",
            Capability::VectorWrite => "vector.write",
            Capability::SqlRead => "sql.read",
            Capability::SqlWrite => "sql.write",
            Capability::WorkflowCall => "workflow.call",
            Capability::ModelCall => "model.call",
            Capability::SchedulerWrite => "scheduler.write",
            Capability::WebhookListen => "webhook.listen",
        }
    }

    /// The default disposition before per-document policy overrides. The two
    /// deny-always capabilities can never be raised above [`CapabilityDisposition::Deny`].
    pub fn default_disposition(&self) -> CapabilityDisposition {
        match self {
            // Never grantable.
            Capability::CredentialExport | Capability::EnvReadSecret => CapabilityDisposition::Deny,
            // Side-effecting / egress powers require explicit arming.
            Capability::ShellExec
            | Capability::NetworkFetch
            | Capability::SqlWrite
            | Capability::SchedulerWrite
            | Capability::WebhookListen
            | Capability::WorkflowCall => CapabilityDisposition::Ask,
            // Sandboxed / read-only powers default to allow.
            Capability::CodeExec
            | Capability::VectorRead
            | Capability::VectorWrite
            | Capability::SqlRead
            | Capability::ModelCall => CapabilityDisposition::Allow,
        }
    }

    /// Whether this capability is deny-always (cannot be granted at any disposition).
    pub fn is_deny_always(&self) -> bool {
        matches!(
            self,
            Capability::CredentialExport | Capability::EnvReadSecret
        )
    }
}

/// How a capability is resolved before runtime guards run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDisposition {
    /// Hard-blocked.
    Deny,
    /// Requires an approval gate.
    Ask,
    /// Permitted (still subject to taint/sandbox/budget guards).
    Allow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_always_capabilities_are_never_grantable() {
        assert!(Capability::CredentialExport.is_deny_always());
        assert!(Capability::EnvReadSecret.is_deny_always());
        assert_eq!(
            Capability::CredentialExport.default_disposition(),
            CapabilityDisposition::Deny
        );
        assert_eq!(
            Capability::EnvReadSecret.default_disposition(),
            CapabilityDisposition::Deny
        );
    }

    #[test]
    fn dotted_ids_round_trip_through_serde() {
        for cap in [
            Capability::ShellExec,
            Capability::CodeExec,
            Capability::NetworkFetch,
            Capability::WorkflowCall,
            Capability::ModelCall,
        ] {
            let json = serde_json::to_string(&cap).unwrap();
            assert_eq!(json, format!("\"{}\"", cap.id()));
            let back: Capability = serde_json::from_str(&json).unwrap();
            assert_eq!(back, cap);
        }
    }

    #[test]
    fn side_effecting_powers_require_arming() {
        assert_eq!(
            Capability::ShellExec.default_disposition(),
            CapabilityDisposition::Ask
        );
        assert_eq!(
            Capability::CodeExec.default_disposition(),
            CapabilityDisposition::Allow
        );
    }
}
