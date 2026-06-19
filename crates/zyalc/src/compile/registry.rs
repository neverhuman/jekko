//! Versioned node/edge registry (F2).
//!
//! `contracts/node-types.json` is the single data-driven source of truth for
//! every ZYAL flow-graph node + edge kind. It is consumed by THREE layers:
//!   * `zyalc` — validate (unknown-kind diagnostics) + hash (`registry_hash`),
//!   * the runner — dispatch by `runtime.executor` (later milestones),
//!   * the web cockpit — render + palette (a byte-identical copy at
//!     `jekko-web/apps/web/src/lib/nodeTypes.json`).
//!
//! Adding a node/edge kind is a registry entry (+ optional adapter), never a
//! renderer or compiler edit (law #1). The registry carries its own
//! `registry_version`; `zyalc migrate` (M17) rewrites docs across bumps.
//!
//! The registry is embedded at compile time so `registry_hash` is reproducible
//! from the binary alone (the M1 determinism envelope reads it from here).

// Several registry fields (runtime/effects/capabilities, migrations, semver
// metadata) are part of the F2 contract surface but only consumed by later
// milestones (M4 authority, M6 lease dispatch). Allow them now.
#![allow(dead_code)]

use std::sync::OnceLock;

use serde::Deserialize;

/// The canonical registry, embedded at build time. The web copy is kept
/// byte-identical (guarded by `flowgraph_registry_matches_web_copy`).
pub(super) const REGISTRY_BYTES: &str = include_str!("../../../../contracts/node-types.json");

/// How a node kind is executed at runtime. The runner dispatches on this.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct RuntimeBinding {
    #[serde(default)]
    pub executor: String,
    #[serde(default)]
    pub tool_ref: Option<String>,
}

/// Declared effects of a node kind. `side_effecting` drives the M4 authority
/// checks (retry needs idempotency; R3+ needs a dominating approval gate).
///
/// IMPORTANT: `default` is a free EFFECT-CATEGORY taxonomy (`model.call`,
/// `network.fetch`, `tool.call`, `memory.write`, `artifact.write`, `render`, …)
/// that M4 maps to an `EffectRisk` (R0–R4). It is deliberately NOT the
/// capability-id vocabulary — an effect describes *what a node does*, a
/// capability is a *grantable power*. Some labels coincide (`model.call`,
/// `network.fetch`) but `tool.call`/`memory.write`/`artifact.write`/`render`
/// are effect categories with no 1:1 grantable capability. Only [`Caps`] holds
/// capability ids; only those are validated against `zyal_core::Capability`.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Effects {
    #[serde(default)]
    pub side_effecting: bool,
    #[serde(default)]
    pub default: Vec<String>,
}

/// Capability requirements — these ARE `zyal_core::Capability` dotted ids and
/// are validated as such (see `capability_ids_are_all_valid`). The runner's
/// lease pipeline (M6) authorizes against them.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Caps {
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
}

/// One registered node kind. Only the fields zyalc/the runner need are
/// deserialized; render/palette/icon metadata is the web's concern and is
/// ignored here (serde drops unknown fields).
#[derive(Debug, Clone, Deserialize)]
pub(super) struct RegisteredNodeKind {
    pub id: String,
    #[serde(default)]
    pub is_container: bool,
    #[serde(default)]
    pub default_edge_kind: String,
    #[serde(default)]
    pub runtime: RuntimeBinding,
    #[serde(default)]
    pub effects: Effects,
    #[serde(default)]
    pub capabilities: Caps,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RegisteredEdgeKind {
    pub kind: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Migration {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
}

/// The parsed registry contract.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Registry {
    #[serde(default)]
    pub registry_id: String,
    #[serde(default)]
    pub registry_version: String,
    #[serde(default)]
    pub schema: String,
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub types: Vec<RegisteredNodeKind>,
    #[serde(default)]
    pub edge_kinds: Vec<RegisteredEdgeKind>,
    #[serde(default)]
    pub migrations: Vec<Migration>,
}

impl Registry {
    pub fn node(&self, id: &str) -> Option<&RegisteredNodeKind> {
        self.types.iter().find(|t| t.id == id)
    }
}

/// Parse the embedded registry. Panics only on a malformed embedded contract
/// (a build-time invariant, caught by `loads_embedded_registry`).
pub(super) fn load_default() -> Registry {
    serde_json::from_str(REGISTRY_BYTES).expect("embedded node-types.json must parse")
}

/// The process-wide registry (parsed once).
pub(super) fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(load_default)
}

/// `sha256:<hex>` of the embedded registry bytes — stamped into the IR's
/// `compile` envelope so a graph is reproducible against a known registry.
pub(super) fn registry_hash() -> String {
    format!("sha256:{}", super::target::sha256(REGISTRY_BYTES))
}

/// Whether `id` is a registered node kind.
pub(super) fn known_node_kind(id: &str) -> bool {
    registry().node(id).is_some()
}

/// The registered kind record (effects/capabilities/runtime) for `id`, or
/// `None` if unregistered. Used by the M4-cont authority checks (`effects.rs`).
pub(super) fn node_kind(id: &str) -> Option<&'static RegisteredNodeKind> {
    registry().node(id)
}

/// The embedded registry's declared `registry_version` (semver, e.g. `1.0.0`).
/// The M4-cont `zyal:` contract-header validator gates a document's required
/// registry version against this.
pub(super) fn registry_version() -> &'static str {
    &registry().registry_version
}

/// Whether `kind` is a registered edge kind.
#[allow(dead_code)] // wired into edge validation in M3
pub(super) fn known_edge_kind(kind: &str) -> bool {
    registry().edge_kinds.iter().any(|e| e.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_embedded_registry() {
        let r = load_default();
        assert_eq!(
            r.schema_version, 2,
            "node-types.json must be schema_version 2"
        );
        assert!(
            !r.registry_version.is_empty(),
            "registry_version must be set"
        );
        assert!(r.types.len() >= 19, "expected the full node-kind catalog");
        // Every kind declares a runtime executor.
        for t in &r.types {
            assert!(
                !t.runtime.executor.is_empty(),
                "node kind `{}` is missing runtime.executor",
                t.id
            );
        }
    }

    #[test]
    fn known_kinds_resolve() {
        for id in [
            "supervisor",
            "agent",
            "router",
            "spawn",
            "fanout",
            "fanin",
            "watcher",
            "kpi",
            "data_feed",
            "artifact",
            "tournament",
            "judge",
        ] {
            assert!(known_node_kind(id), "expected `{id}` to be registered");
        }
        assert!(!known_node_kind("definitely_not_a_kind"));
        assert!(known_edge_kind("feeds") && known_edge_kind("loop_back"));
    }

    #[test]
    fn registry_hash_is_stable_and_prefixed() {
        let a = registry_hash();
        let b = registry_hash();
        assert_eq!(a, b);
        assert!(a.starts_with("sha256:") && a.len() == "sha256:".len() + 64);
    }

    #[test]
    fn capability_ids_are_all_valid() {
        // Every id in capabilities.{requires,optional,forbidden} must be a real
        // `zyal_core::Capability` so the M6 lease pipeline never sees a bogus id.
        // (Effects.default is a DIFFERENT vocabulary and is intentionally NOT
        // checked here — see the Effects/Caps docs.)
        let r = load_default();
        for t in &r.types {
            for (field, ids) in [
                ("requires", &t.capabilities.requires),
                ("optional", &t.capabilities.optional),
                ("forbidden", &t.capabilities.forbidden),
            ] {
                for id in ids {
                    let parsed: Result<zyal_core::Capability, _> =
                        serde_json::from_str(&format!("\"{id}\""));
                    assert!(
                        parsed.is_ok(),
                        "node `{}` capabilities.{field} has invalid capability id `{id}` (not a zyal_core::Capability)",
                        t.id
                    );
                }
            }
        }
    }
}
