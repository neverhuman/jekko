//! Authority-as-topology (M4-cont).
//!
//! Every node declares effects (from its registry kind, overridable per-node).
//! We map effect categories to an [`EffectRisk`] (R0 pure → R4 arbitrary host
//! exec) and enforce three STRUCTURAL authority laws on the *built* FlowGraph —
//! authority is a property of the topology, not a runtime afterthought:
//!
//!   * `ZYAL_E_RETRY_WITHOUT_IDEMPOTENCY` — a side-effecting, non-idempotent
//!     node may not declare a retry policy (a retried side effect double-acts).
//!   * `ZYAL_E_EFFECT_NEEDS_APPROVAL` — an R3+ (host-mutating / exec) node must
//!     be DOMINATED by an approval/gate node on every path from a root: you
//!     cannot reach a dangerous effect without first passing an approval.
//!   * `ZYAL_E_CAPABILITY_DENIED` — a node may not require a deny-always
//!     capability (`credential.export` / `env.read_secret`) nor one its kind
//!     forbids.
//!
//! Read egress (`network.fetch` for research/feeds) is R2, NOT host-arming, so
//! it never needs an approval gate — only durable host mutation / exec does.
//! This keeps research roots (a `web_search` with no upstream gate) valid.

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};
use zyal_core::Capability;

/// Risk tier of a node's declared effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum EffectRisk {
    /// Pure / visual-only.
    R0,
    /// Read / compute (model.call, render, read queries).
    R1,
    /// Read egress + durable sandbox writes — elevated, NOT host-arming.
    R2,
    /// External host mutation / side effects — needs a dominating approval.
    R3,
    /// Arbitrary host execution.
    R4,
}

/// R3+ effects must be dominated by an approval gate.
const APPROVAL_FLOOR: EffectRisk = EffectRisk::R3;

/// Map a single effect category to its risk tier. The taxonomy is the registry
/// `effects.default` vocabulary (see `registry::Effects` docs).
fn category_risk(cat: &str) -> EffectRisk {
    match cat {
        "model.call" | "render" | "vector.read" | "sql.read" => EffectRisk::R1,
        "network.fetch" | "memory.write" | "artifact.write" | "vector.write" => EffectRisk::R2,
        "tool.call" | "sql.write" | "scheduler.write" | "webhook.listen" | "workflow.call" => {
            EffectRisk::R3
        }
        "shell.exec" => EffectRisk::R4,
        // Unknown effect category: treat as low-risk compute (lenient).
        _ => EffectRisk::R1,
    }
}

fn node_type(node: &Map<String, Value>) -> &str {
    node.get("node_type").and_then(Value::as_str).unwrap_or("")
}

fn node_id(node: &Map<String, Value>) -> &str {
    node.get("id").and_then(Value::as_str).unwrap_or("<node>")
}

/// A node's effect categories: an authored `effects.default` override wins,
/// else the registry kind's declared categories.
fn effect_categories(node: &Map<String, Value>) -> Vec<String> {
    if let Some(cats) = node
        .get("effects")
        .and_then(Value::as_object)
        .and_then(|e| e.get("default"))
        .and_then(Value::as_array)
    {
        return cats
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    super::registry::node_kind(node_type(node))
        .map(|k| k.effects.default.clone())
        .unwrap_or_default()
}

fn side_effecting(node: &Map<String, Value>) -> bool {
    if let Some(b) = node
        .get("effects")
        .and_then(Value::as_object)
        .and_then(|e| e.get("side_effecting"))
        .and_then(Value::as_bool)
    {
        return b;
    }
    super::registry::node_kind(node_type(node))
        .map(|k| k.effects.side_effecting)
        .unwrap_or(false)
}

fn risk(node: &Map<String, Value>) -> EffectRisk {
    effect_categories(node)
        .iter()
        .map(|c| category_risk(c))
        .max()
        .unwrap_or(EffectRisk::R0)
}

fn required_caps(node: &Map<String, Value>) -> Vec<String> {
    if let Some(reqs) = node
        .get("capabilities")
        .and_then(Value::as_object)
        .and_then(|c| c.get("requires"))
        .and_then(Value::as_array)
    {
        return reqs
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    super::registry::node_kind(node_type(node))
        .map(|k| k.capabilities.requires.clone())
        .unwrap_or_default()
}

/// Forbidden capabilities = the kind's declared `forbidden` set ∪ any authored
/// `capabilities.forbidden`.
fn forbidden_caps(node: &Map<String, Value>) -> Vec<String> {
    let mut f = super::registry::node_kind(node_type(node))
        .map(|k| k.capabilities.forbidden.clone())
        .unwrap_or_default();
    if let Some(extra) = node
        .get("capabilities")
        .and_then(Value::as_object)
        .and_then(|c| c.get("forbidden"))
        .and_then(Value::as_array)
    {
        f.extend(extra.iter().filter_map(|v| v.as_str().map(str::to_string)));
    }
    f
}

fn is_idempotent(node: &Map<String, Value>) -> bool {
    match node.get("idempotency").and_then(Value::as_str) {
        Some("idempotent") => true,
        Some(_) => false,
        None => node
            .get("idempotent")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn declares_retry(node: &Map<String, Value>) -> bool {
    match node.get("retry") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => true,
    }
}

/// Whether a node is an approval/gate node that can dominate a dangerous effect.
fn is_gate(node: &Map<String, Value>) -> bool {
    let nt = node_type(node);
    if nt.ends_with("_gate") || matches!(nt, "approval" | "human_gate") {
        return true;
    }
    ["approval", "requires_approval", "gate"]
        .iter()
        .any(|flag| node.get(*flag).and_then(Value::as_bool) == Some(true))
}

/// Run all three authority laws over the built nodes/edges.
pub(super) fn validate_effects(nodes: &[Value], edges: &[Value]) -> Result<()> {
    // Pass 1: per-node capability + retry/idempotency laws.
    for n in nodes {
        let Some(nm) = n.as_object() else { continue };
        let forbidden = forbidden_caps(nm);
        for cap_id in required_caps(nm) {
            if let Ok(cap) = serde_json::from_str::<Capability>(&format!("\"{cap_id}\"")) {
                if cap.is_deny_always() {
                    return Err(anyhow!(
                        "ZYAL_E_CAPABILITY_DENIED: node `{}` requires deny-always capability `{cap_id}` (never grantable at any disposition)",
                        node_id(nm)
                    ));
                }
            }
            if forbidden.iter().any(|f| f == &cap_id) {
                return Err(anyhow!(
                    "ZYAL_E_CAPABILITY_DENIED: node `{}` requires capability `{cap_id}` which its kind forbids",
                    node_id(nm)
                ));
            }
        }
        if declares_retry(nm)
            && side_effecting(nm)
            && risk(nm) >= EffectRisk::R2
            && !is_idempotent(nm)
        {
            return Err(anyhow!(
                "ZYAL_E_RETRY_WITHOUT_IDEMPOTENCY: node `{}` is side-effecting and non-idempotent but declares a `retry` policy (a retried side effect double-acts; declare `idempotency: idempotent` or drop `retry`)",
                node_id(nm)
            ));
        }
    }
    // Pass 2: R3+ effects must be dominated by an approval gate.
    validate_approval_domination(nodes, edges)
}

/// Enforce that every R3+ node is dominated by a gate: with all gate nodes
/// removed, no R3+ node may remain reachable from a graph root. (If a high-risk
/// node is still reachable, there is a gate-free path to it — not dominated.)
fn validate_approval_domination(nodes: &[Value], edges: &[Value]) -> Result<()> {
    let mut gates: HashSet<&str> = HashSet::new();
    let mut high_risk: Vec<(&str, &str)> = Vec::new(); // (id, kind)
    let mut ids: HashSet<&str> = HashSet::new();
    for n in nodes {
        let Some(nm) = n.as_object() else { continue };
        let id = node_id(nm);
        ids.insert(id);
        if is_gate(nm) {
            gates.insert(id);
        }
        if risk(nm) >= APPROVAL_FLOOR {
            high_risk.push((id, node_type(nm)));
        }
    }
    if high_risk.is_empty() {
        return Ok(());
    }

    // Adjacency over structural (non-loop_back) edges. Indegree over the full
    // graph identifies roots; traversal skips gate nodes (removing them cuts
    // every path through them).
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut indeg: HashMap<&str, usize> = ids.iter().map(|id| (*id, 0usize)).collect();
    for e in edges {
        let Some(em) = e.as_object() else { continue };
        let is_back = em.get("loop_back").and_then(Value::as_bool) == Some(true)
            || em.get("kind").and_then(Value::as_str) == Some("loop_back");
        if is_back {
            continue;
        }
        let (Some(src), Some(dst)) = (
            em.get("src").and_then(Value::as_str),
            em.get("dst").and_then(Value::as_str),
        ) else {
            continue;
        };
        *indeg.entry(dst).or_insert(0) += 1;
        adj.entry(src).or_default().push(dst);
    }

    // BFS from non-gate roots, never entering a gate node.
    let mut reachable: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    for (id, deg) in &indeg {
        if *deg == 0 && !gates.contains(id) && reachable.insert(*id) {
            queue.push_back(*id);
        }
    }
    while let Some(cur) = queue.pop_front() {
        if let Some(children) = adj.get(cur) {
            for &c in children {
                if gates.contains(c) {
                    continue; // a gate dominates everything past it
                }
                if reachable.insert(c) {
                    queue.push_back(c);
                }
            }
        }
    }

    for (id, kind) in high_risk {
        if gates.contains(id) {
            // A dangerous effect may not be its own dominator. If a node could be
            // both R3+ AND a gate, an author could self-exempt it (tag a `tool`
            // node `approval: true` and it would never be externally approved).
            // Approval gates must be low-risk (R0–R2).
            return Err(anyhow!(
                "ZYAL_E_EFFECT_NEEDS_APPROVAL: node `{id}` (kind `{kind}`) is marked as an approval/gate but performs an R3+ host-mutating/exec effect — an approval gate must be low-risk (R0–R2); use a separate gate node to dominate `{id}`"
            ));
        }
        if reachable.contains(id) {
            return Err(anyhow!(
                "ZYAL_E_EFFECT_NEEDS_APPROVAL: node `{id}` (kind `{kind}`) performs an R3+ host-mutating/exec effect but is reachable from a graph root without passing an approval gate — insert a gate (a `*_gate`/`human_gate` node, or `approval: true`) that dominates it on every path"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn nodes(v: Value) -> Vec<Value> {
        v.as_array().unwrap().clone()
    }

    #[test]
    fn pure_graph_passes() {
        let ns = nodes(json!([
            { "id": "a", "node_type": "agent" },
            { "id": "b", "node_type": "judge" },
        ]));
        validate_effects(&ns, &[]).unwrap();
    }

    #[test]
    fn read_egress_root_needs_no_gate() {
        // web_search is side_effecting network.fetch (R2) — a research root must
        // remain valid without an upstream approval.
        let ns = nodes(json!([{ "id": "research", "node_type": "web_search" }]));
        validate_effects(&ns, &[]).unwrap();
    }

    #[test]
    fn ungated_host_action_is_rejected() {
        let ns = nodes(json!([
            { "id": "a", "node_type": "agent" },
            { "id": "t", "node_type": "tool" },
        ]));
        let edges = nodes(json!([{ "src": "a", "dst": "t", "kind": "feeds" }]));
        let err = validate_effects(&ns, &edges).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_EFFECT_NEEDS_APPROVAL"),
            "got {err}"
        );
    }

    #[test]
    fn gated_host_action_passes() {
        let ns = nodes(json!([
            { "id": "a", "node_type": "agent" },
            { "id": "g", "node_type": "budget_gate" },
            { "id": "t", "node_type": "tool" },
        ]));
        let edges = nodes(json!([
            { "src": "a", "dst": "g", "kind": "feeds" },
            { "src": "g", "dst": "t", "kind": "feeds" },
        ]));
        validate_effects(&ns, &edges).unwrap();
    }

    #[test]
    fn deny_always_capability_is_rejected() {
        let ns = nodes(json!([
            { "id": "x", "node_type": "agent", "capabilities": { "requires": ["credential.export"] } },
        ]));
        let err = validate_effects(&ns, &[]).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_CAPABILITY_DENIED"),
            "got {err}"
        );
    }

    #[test]
    fn forbidden_capability_is_rejected() {
        // paper_builder forbids network.fetch.
        let ns = nodes(json!([
            { "id": "p", "node_type": "paper_builder", "capabilities": { "requires": ["network.fetch"] } },
        ]));
        let err = validate_effects(&ns, &[]).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_CAPABILITY_DENIED"),
            "got {err}"
        );
    }

    #[test]
    fn retry_on_non_idempotent_side_effect_is_rejected() {
        let ns = nodes(json!([
            { "id": "w", "node_type": "artifact", "retry": { "max": 3 } },
        ]));
        let err = validate_effects(&ns, &[]).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_RETRY_WITHOUT_IDEMPOTENCY"),
            "got {err}"
        );
    }

    #[test]
    fn retry_with_declared_idempotency_passes() {
        let ns = nodes(json!([
            { "id": "w", "node_type": "artifact", "retry": { "max": 3 }, "idempotency": "idempotent" },
        ]));
        validate_effects(&ns, &[]).unwrap();
    }

    #[test]
    fn high_risk_node_marked_as_gate_is_rejected() {
        // A dangerous effect cannot self-exempt by tagging itself an approval gate.
        let ns = nodes(json!([{ "id": "g", "node_type": "tool", "approval": true }]));
        let err = validate_effects(&ns, &[]).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_EFFECT_NEEDS_APPROVAL"),
            "got {err}"
        );
    }

    #[test]
    fn gate_typed_node_with_authored_host_effect_is_rejected() {
        // A `*_gate` kind carrying an authored R4 effect is still self-domination.
        let ns = nodes(json!([
            { "id": "g", "node_type": "budget_gate", "effects": { "default": ["shell.exec"], "side_effecting": true } },
        ]));
        let err = validate_effects(&ns, &[]).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_EFFECT_NEEDS_APPROVAL"),
            "got {err}"
        );
    }

    #[test]
    fn gate_only_root_does_not_make_action_reachable() {
        // gate is the only root; the action is dominated by it.
        let ns = nodes(json!([
            { "id": "g", "node_type": "human_gate" },
            { "id": "t", "node_type": "tool" },
        ]));
        let edges = nodes(json!([{ "src": "g", "dst": "t", "kind": "feeds" }]));
        validate_effects(&ns, &edges).unwrap();
    }
}
