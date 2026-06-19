//! Small structural type system for typed + taint edges (F3).
//!
//! Edges become contracts checked at compile time. The system is **gradual**:
//! anything untyped is `Any` (top AND bottom), so existing untyped graphs keep
//! compiling — a diagnostic only fires when BOTH endpoints carry concrete,
//! declared, incompatible types, or when an edge carries taint the target
//! cannot safely consume (law #6).
//!
//! Authoring surface (all optional):
//!   * top-level `types:` — named type definitions (string exprs or objects),
//!   * per-node `port_types: { in: { <port>: <expr> }, out: { <port>: <expr> } }`,
//!   * per-edge `payload_type:`, `taint: [<label>...]`, `sanitizer:`,
//!   * `from: node.port` / `to: node.port` (or explicit `from_port`/`to_port`).
//!
//! Type expressions: `any`, scalars (`string|number|int|float|bool`),
//! generics `stream[T] | set[T] | list[T]`, unions `A | B`, named refs into
//! `types:`, and object types (from the `types:` block).

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

use zyal_core::{ActionClass, TaintLabel, TaintSet};

const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ZyalType {
    Any,
    Scalar(String),
    Named(String),
    Union(Vec<ZyalType>),
    Generic {
        ctor: String,
        arg: Box<ZyalType>,
    },
    Object {
        required: Vec<String>,
        properties: BTreeMap<String, ZyalType>,
    },
}

impl ZyalType {
    /// Human label for diagnostics.
    fn display(&self) -> String {
        match self {
            ZyalType::Any => "any".into(),
            ZyalType::Scalar(s) => s.clone(),
            ZyalType::Named(n) => n.clone(),
            ZyalType::Union(ms) => ms
                .iter()
                .map(ZyalType::display)
                .collect::<Vec<_>>()
                .join(" | "),
            ZyalType::Generic { ctor, arg } => format!("{ctor}[{}]", arg.display()),
            ZyalType::Object { required, .. } => format!("object{{{}}}", required.join(",")),
        }
    }

    fn is_concrete(&self) -> bool {
        !matches!(self, ZyalType::Any)
    }
}

/// Named type environment from the top-level `types:` block.
#[derive(Debug, Default)]
pub(super) struct TypeEnv {
    named: BTreeMap<String, ZyalType>,
}

impl TypeEnv {
    fn resolve<'a>(&'a self, t: &'a ZyalType) -> &'a ZyalType {
        match t {
            ZyalType::Named(n) => self.named.get(n).unwrap_or(t),
            _ => t,
        }
    }

    /// Structural assignability with `Any` as top+bottom (gradual). `from` is
    /// assignable to `into` when a value of `from` can flow into `into`.
    pub(super) fn is_assignable_to(&self, from: &ZyalType, into: &ZyalType) -> bool {
        self.assignable(from, into, 0)
    }

    fn assignable(&self, from: &ZyalType, into: &ZyalType, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return true; // give up safely on pathological recursion
        }
        let from = self.resolve(from);
        let into = self.resolve(into);
        if matches!(from, ZyalType::Any) || matches!(into, ZyalType::Any) {
            return true;
        }
        match (from, into) {
            // A union source is assignable only if EVERY member is.
            (ZyalType::Union(ms), _) => ms.iter().all(|m| self.assignable(m, into, depth + 1)),
            // Into a union: assignable if it matches SOME member.
            (_, ZyalType::Union(ms)) => ms.iter().any(|m| self.assignable(from, m, depth + 1)),
            (ZyalType::Scalar(a), ZyalType::Scalar(b)) => a == b,
            (ZyalType::Generic { ctor: c1, arg: a1 }, ZyalType::Generic { ctor: c2, arg: a2 }) => {
                c1 == c2 && self.assignable(a1, a2, depth + 1)
            }
            (
                ZyalType::Object { properties: pa, .. },
                ZyalType::Object {
                    required: rb,
                    properties: pb,
                },
            ) => rb.iter().all(|k| {
                pa.get(k)
                    .zip(pb.get(k))
                    .map(|(av, bv)| self.assignable(av, bv, depth + 1))
                    .unwrap_or(false)
            }),
            (ZyalType::Named(a), ZyalType::Named(b)) => a == b,
            _ => false,
        }
    }
}

/// Parse a type expression string into a [`ZyalType`]. Never fails — an
/// unrecognized token becomes a `Named` ref (resolved against the env, opaque
/// otherwise).
fn parse_type_expr(raw: &str) -> ZyalType {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("any") {
        return ZyalType::Any;
    }
    // Top-level union (split on `|`, not inside brackets).
    if let Some(parts) = split_top_level_union(s) {
        return ZyalType::Union(parts.iter().map(|p| parse_type_expr(p)).collect());
    }
    // Generic `ctor[arg]`.
    if let Some((ctor, inner)) = parse_generic(s) {
        if matches!(ctor.as_str(), "stream" | "set" | "list") {
            return ZyalType::Generic {
                ctor,
                arg: Box::new(parse_type_expr(&inner)),
            };
        }
    }
    match s {
        "string" | "str" => ZyalType::Scalar("string".into()),
        "number" | "num" | "float" => ZyalType::Scalar("number".into()),
        "int" | "integer" => ZyalType::Scalar("int".into()),
        "bool" | "boolean" => ZyalType::Scalar("bool".into()),
        other => ZyalType::Named(other.to_string()),
    }
}

fn split_top_level_union(s: &str) -> Option<Vec<String>> {
    let mut depth = 0i32;
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut saw = false;
    for ch in s.chars() {
        match ch {
            '[' | '(' => {
                depth += 1;
                cur.push(ch);
            }
            ']' | ')' => {
                depth -= 1;
                cur.push(ch);
            }
            '|' if depth == 0 => {
                saw = true;
                parts.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !saw {
        return None;
    }
    parts.push(cur.trim().to_string());
    Some(parts.into_iter().filter(|p| !p.is_empty()).collect())
}

fn parse_generic(s: &str) -> Option<(String, String)> {
    let open = s.find('[')?;
    if !s.ends_with(']') {
        return None;
    }
    let ctor = s[..open].trim().to_string();
    let inner = s[open + 1..s.len() - 1].to_string();
    if ctor.is_empty() {
        return None;
    }
    Some((ctor, inner))
}

/// Build the named-type environment from the top-level `types:` block.
fn build_env(root: &Map<String, Value>) -> TypeEnv {
    let mut named = BTreeMap::new();
    if let Some(types) = root.get("types").and_then(Value::as_object) {
        for (name, def) in types {
            named.insert(name.clone(), parse_type_def(def));
        }
    }
    TypeEnv { named }
}

fn parse_type_def(def: &Value) -> ZyalType {
    match def {
        Value::String(s) => parse_type_expr(s),
        Value::Object(o) => {
            // Object type: `{ properties: { k: expr }, required: [k] }` or a bare
            // `{ k: expr }` (all keys required).
            if let Some(props) = o.get("properties").and_then(Value::as_object) {
                let required = o
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_else(|| props.keys().cloned().collect());
                let properties = props
                    .iter()
                    .map(|(k, v)| (k.clone(), parse_type_def(v)))
                    .collect();
                ZyalType::Object {
                    required,
                    properties,
                }
            } else if let Some(union) = o.get("union").and_then(Value::as_array) {
                ZyalType::Union(union.iter().map(parse_type_def).collect())
            } else {
                let properties: BTreeMap<String, ZyalType> = o
                    .iter()
                    .map(|(k, v)| (k.clone(), parse_type_def(v)))
                    .collect();
                let required = properties.keys().cloned().collect();
                ZyalType::Object {
                    required,
                    properties,
                }
            }
        }
        _ => ZyalType::Any,
    }
}

/// `(node_id, port)` from a `node.port` reference, or `(whole, None)`.
fn split_ref(s: &str) -> (String, Option<String>) {
    match s.split_once('.') {
        Some((node, port)) if !port.is_empty() => (node.to_string(), Some(port.to_string())),
        _ => (s.to_string(), None),
    }
}

/// Per-node declared port types: `in`/`out` maps of port name → type expr.
struct NodePortTypes {
    inputs: BTreeMap<String, ZyalType>,
    outputs: BTreeMap<String, ZyalType>,
}

fn node_port_types(node: &Map<String, Value>) -> NodePortTypes {
    let read = |dir: &str| -> BTreeMap<String, ZyalType> {
        node.get("port_types")
            .and_then(Value::as_object)
            .and_then(|pt| pt.get(dir))
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), parse_type_expr(s))))
                    .collect()
            })
            .unwrap_or_default()
    };
    NodePortTypes {
        inputs: read("in"),
        outputs: read("out"),
    }
}

/// Map a target node kind to the action class its taint gate guards. `model.call`
/// (agent/judge) is `RemoteModelCall` even when not host-side-effecting — that is
/// the secret-leak vector. Returns `None` when the kind performs no gated action.
fn action_class_for(node_type: &str) -> Option<ActionClass> {
    let n = super::registry::registry().node(node_type)?;
    let effects = &n.effects.default;
    if n.runtime.executor == "agent" || effects.iter().any(|e| e == "model.call") {
        return Some(ActionClass::RemoteModelCall);
    }
    if !n.effects.side_effecting {
        return None;
    }
    if effects.iter().any(|e| e == "memory.write") {
        return Some(ActionClass::WriteDurableMemory);
    }
    if effects.iter().any(|e| e == "shell.exec") {
        return Some(ActionClass::ExecShell);
    }
    Some(ActionClass::ArmHostAction)
}

fn taint_set(labels: &[Value], sanitizer: Option<&str>) -> TaintSet {
    let mut set = TaintSet::default();
    for l in labels {
        if let Some(s) = l.as_str() {
            if let Ok(label) = serde_json::from_value::<TaintLabel>(Value::String(s.to_string())) {
                set.labels.insert(label);
            }
        }
    }
    if let Some(name) = sanitizer {
        zyal_core::transfer_taint(&set, Some(name))
    } else {
        set
    }
}

/// Validate typed + taint edges on an authored graph. No-op when there is no
/// authored `graph.nodes`. Gradual: only concrete, declared, incompatible types
/// (or unsafe taint) raise a diagnostic.
pub(super) fn validate_typed_graph(root: &Map<String, Value>) -> Result<()> {
    let Some(graph) = root.get("graph").and_then(Value::as_object) else {
        return Ok(());
    };
    let Some(nodes) = graph.get("nodes").and_then(Value::as_array) else {
        return Ok(());
    };
    let env = build_env(root);

    // Index node kind + declared port types by id.
    let mut kind_by_id: BTreeMap<String, String> = BTreeMap::new();
    let mut ports_by_id: BTreeMap<String, NodePortTypes> = BTreeMap::new();
    for n in nodes {
        let Some(nm) = n.as_object() else { continue };
        let Some(id) = nm.get("id").and_then(Value::as_str) else {
            continue;
        };
        let ty = nm.get("type").and_then(Value::as_str).unwrap_or("agent");
        kind_by_id.insert(id.to_string(), ty.to_string());
        ports_by_id.insert(id.to_string(), node_port_types(nm));
    }

    let Some(edges) = graph.get("edges").and_then(Value::as_array) else {
        return Ok(());
    };
    for e in edges {
        let Some(em) = e.as_object() else { continue };
        let raw_from = em
            .get("from")
            .or_else(|| em.get("src"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let raw_to = em
            .get("to")
            .or_else(|| em.get("dst"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if raw_from.is_empty() || raw_to.is_empty() {
            continue; // structural validator already covers this
        }
        let (src_id, src_port_ref) = split_ref(raw_from);
        let (dst_id, dst_port_ref) = split_ref(raw_to);
        let from_port = em
            .get("from_port")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(src_port_ref);
        let to_port = em
            .get("to_port")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(dst_port_ref);

        // ---- type check (gradual) ----
        let src_type = from_port
            .as_ref()
            .and_then(|p| ports_by_id.get(&src_id).and_then(|pt| pt.outputs.get(p)))
            .cloned()
            .or_else(|| {
                em.get("payload_type")
                    .and_then(Value::as_str)
                    .map(parse_type_expr)
            })
            .unwrap_or(ZyalType::Any);
        let dst_type = to_port
            .as_ref()
            .and_then(|p| ports_by_id.get(&dst_id).and_then(|pt| pt.inputs.get(p)))
            .cloned()
            .unwrap_or(ZyalType::Any);
        if src_type.is_concrete()
            && dst_type.is_concrete()
            && !env.is_assignable_to(&src_type, &dst_type)
        {
            return Err(anyhow!(
                "ZYAL_E_EDGE_TYPE_MISMATCH: Cannot connect {raw_from}: {} to {raw_to}: {}.",
                src_type.display(),
                dst_type.display()
            ));
        }

        // ---- taint check (law #6) ----
        if let Some(taint) = em.get("taint").and_then(Value::as_array) {
            if !taint.is_empty() {
                let sanitizer = em.get("sanitizer").and_then(Value::as_str);
                let set = taint_set(taint, sanitizer);
                if let Some(target_kind) = kind_by_id.get(&dst_id) {
                    if let Some(action) = action_class_for(target_kind) {
                        if !set.can_arm(action) {
                            return Err(anyhow!(
                                "ZYAL_E_TAINT_VIOLATION: tainted data on edge {raw_from} → {raw_to} cannot {:?} at `{dst_id}` (declare a `sanitizer:` on the edge)",
                                action
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env() -> TypeEnv {
        TypeEnv::default()
    }

    #[test]
    fn any_is_top_and_bottom() {
        let e = env();
        assert!(e.is_assignable_to(&ZyalType::Any, &ZyalType::Scalar("string".into())));
        assert!(e.is_assignable_to(&ZyalType::Scalar("string".into()), &ZyalType::Any));
    }

    #[test]
    fn scalars_match_exactly() {
        let e = env();
        assert!(e.is_assignable_to(&parse_type_expr("string"), &parse_type_expr("string")));
        assert!(!e.is_assignable_to(&parse_type_expr("string"), &parse_type_expr("number")));
    }

    #[test]
    fn generics_are_covariant_by_ctor() {
        let e = env();
        assert!(e.is_assignable_to(
            &parse_type_expr("list[string]"),
            &parse_type_expr("list[string]")
        ));
        assert!(!e.is_assignable_to(
            &parse_type_expr("list[string]"),
            &parse_type_expr("set[string]")
        ));
        assert!(!e.is_assignable_to(
            &parse_type_expr("list[string]"),
            &parse_type_expr("list[number]")
        ));
        assert!(e.is_assignable_to(
            &parse_type_expr("list[string]"),
            &parse_type_expr("list[any]")
        ));
    }

    #[test]
    fn unions_distribute() {
        let e = env();
        // into a union: ok if it matches a member
        assert!(e.is_assignable_to(
            &parse_type_expr("string"),
            &parse_type_expr("string | number")
        ));
        // a union source: ok only if every member is assignable
        assert!(e.is_assignable_to(
            &parse_type_expr("string | number"),
            &parse_type_expr("string | number")
        ));
        assert!(!e.is_assignable_to(
            &parse_type_expr("string | bool"),
            &parse_type_expr("string | number")
        ));
    }

    #[test]
    fn objects_use_width_depth_subtyping() {
        let mut named = BTreeMap::new();
        named.insert(
            "Wide".to_string(),
            parse_type_def(&json!({"properties": {"a": "string", "b": "number"}})),
        );
        named.insert(
            "Narrow".to_string(),
            parse_type_def(&json!({"properties": {"a": "string"}, "required": ["a"]})),
        );
        let e = TypeEnv { named };
        // Wide has a+b, satisfies Narrow's required {a}
        assert!(e.is_assignable_to(
            &ZyalType::Named("Wide".into()),
            &ZyalType::Named("Narrow".into())
        ));
        // Narrow lacks b, cannot satisfy Wide's required {a,b}
        assert!(!e.is_assignable_to(
            &ZyalType::Named("Narrow".into()),
            &ZyalType::Named("Wide".into())
        ));
    }

    #[test]
    fn untyped_graph_is_a_noop() {
        let root = json!({
            "graph": { "nodes": [
                {"id": "a", "type": "agent"},
                {"id": "b", "type": "agent"}
            ], "edges": [
                {"from": "a", "to": "b", "kind": "feeds"}
            ]}
        });
        validate_typed_graph(root.as_object().unwrap()).unwrap();
    }

    #[test]
    fn declared_mismatch_is_rejected() {
        let root = json!({
            "types": {"Draft": "string", "Page": "number"},
            "graph": { "nodes": [
                {"id": "a", "type": "agent", "port_types": {"out": {"result": "Page"}}},
                {"id": "b", "type": "agent", "port_types": {"in": {"prompt": "Draft"}}}
            ], "edges": [
                {"from": "a.result", "to": "b.prompt", "kind": "feeds"}
            ]}
        });
        let err = validate_typed_graph(root.as_object().unwrap()).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_EDGE_TYPE_MISMATCH"),
            "got {err}"
        );
    }

    #[test]
    fn compatible_declared_types_pass() {
        let root = json!({
            "graph": { "nodes": [
                {"id": "a", "type": "agent", "port_types": {"out": {"result": "string"}}},
                {"id": "b", "type": "agent", "port_types": {"in": {"prompt": "string"}}}
            ], "edges": [
                {"from": "a.result", "to": "b.prompt", "kind": "feeds"}
            ]}
        });
        validate_typed_graph(root.as_object().unwrap()).unwrap();
    }

    #[test]
    fn secret_into_remote_model_is_a_taint_violation() {
        let root = json!({
            "graph": { "nodes": [
                {"id": "src", "type": "data_feed"},
                {"id": "model", "type": "agent"}
            ], "edges": [
                {"from": "src", "to": "model", "kind": "feeds", "taint": ["secret"]}
            ]}
        });
        let err = validate_typed_graph(root.as_object().unwrap()).unwrap_err();
        assert!(
            format!("{err}").contains("ZYAL_E_TAINT_VIOLATION"),
            "got {err}"
        );
    }

    #[test]
    fn sanitizer_clears_web_content_taint() {
        // untrusted web content into a side-effecting tool: blocked raw...
        let blocked = json!({
            "graph": { "nodes": [
                {"id": "src", "type": "web_search"},
                {"id": "sink", "type": "data_feed"}
            ], "edges": [
                {"from": "src", "to": "sink", "kind": "feeds", "taint": ["web_content"]}
            ]}
        });
        assert!(validate_typed_graph(blocked.as_object().unwrap()).is_err());
        // ...allowed once a sanitizer is declared.
        let ok = json!({
            "graph": { "nodes": [
                {"id": "src", "type": "web_search"},
                {"id": "sink", "type": "data_feed"}
            ], "edges": [
                {"from": "src", "to": "sink", "kind": "feeds", "taint": ["web_content"], "sanitizer": "html_strip"}
            ]}
        });
        validate_typed_graph(ok.as_object().unwrap()).unwrap();
    }
}
