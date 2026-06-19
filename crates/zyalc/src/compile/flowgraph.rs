//! FlowGraph IR builder (`target=flowgraph`).
//!
//! Emits a fully-flattened nodes+edges agent-flow graph that the jekko-web
//! diagram consumes directly with zero topology inference. Fan-out, jailgun
//! spawns, jnoccio routing, and dispatch lanes are EXPANDED into concrete node
//! instances (`route/draft-0..N`, `jailgun/tab-1..N`, `worker-pool/<id>/w-0..N`).
//!
//! Loops/generations are NOT a visual axis. Iteration renders as either:
//!   * a `loop_back` edge from the end of a body back to its entry (default), or
//!   * an unrolled body repeated N times (`unroll`, or `auto` when N is small).
//!
//! Two input modes:
//!   * AUTHORED — an explicit `graph: { nodes: [...], edges: [...] }` block is
//!     authoritative; cardinality on `router`/`spawn`/`fanout` nodes is expanded.
//!   * SYNTHESIZED — no authored graph; the IR is derived best-effort from the
//!     policy blocks (`agents`, `fan_out`, `dispatch`, `hero_judge`) plus the
//!     capability blocks (`watchers`, `kpi`, `data_feeds`, `artifacts`).

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

const SCHEMA: &str = "zyal/flowgraph@1";
/// Deterministic IR envelope version (law #2). Bumped independently of the
/// `schema` tag whenever the IR shape changes; the runner gates `min_runner`
/// against this.
const IR_VERSION: &str = "flowgraph.v3";
/// `auto` unroll materializes the body when iteration count is at or below this.
const AUTO_UNROLL_MAX: u64 = 4;

mod paper_builder;

pub(super) fn build_flowgraph(
    parsed: &serde_yaml::Value,
    body: &str,
    uses: &super::uses::ResolvedUses,
) -> Result<Value> {
    let root_json = serde_json::to_value(parsed)
        .map_err(|e| anyhow!("flowgraph YAML→JSON conversion failed: {e}"))?;
    let root = root_json
        .as_object()
        .ok_or_else(|| anyhow!("flowgraph body must be a mapping"))?;

    // F3: typed + taint edge contracts (gradual — no-op on untyped graphs).
    super::types::validate_typed_graph(root)?;
    // F1b: dynamic cardinality bounds + identity (no-op on resolved graphs).
    super::cardinality::validate_cardinality(root)?;

    let id = root
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("flowgraph")
        .to_string();
    let job = root.get("job").cloned().unwrap_or_else(|| json!({}));

    let graph = root.get("graph").and_then(Value::as_object);
    let unroll = graph
        .and_then(|g| g.get("unroll"))
        .and_then(Value::as_object);
    let unroll_loops = unroll
        .and_then(|u| u.get("loops"))
        .and_then(Value::as_str)
        .unwrap_or("backedge")
        .to_string();
    let unroll_generations = unroll
        .and_then(|u| u.get("generations"))
        .and_then(Value::as_str)
        .unwrap_or("window")
        .to_string();

    let mut b = Builder::new(unroll_loops.clone());

    let authored_nodes = graph.and_then(|g| g.get("nodes")).and_then(Value::as_array);
    if let Some(nodes) = authored_nodes {
        let edges = graph.and_then(|g| g.get("edges")).and_then(Value::as_array);
        b.from_authored(nodes, edges);
    } else {
        b.synthesize(root);
        b.capability_blocks(root);
    }
    b.paper_builder_block(root, uses)?;

    // Pattern compiler (M7): lower a top-level `patterns:` block into concrete
    // FlowGraph nodes/edges. Additive — coexists with authored or synthesized.
    // Validate bounds FIRST (bounded-by-construction also holds for synthesized
    // pattern nodes, which `validate_cardinality` doesn't reach).
    let patterns = super::patterns::parse_patterns(root);
    super::patterns::validate_patterns(&patterns)?;
    b.emit_patterns(&patterns);

    // M4-cont: authority-as-topology — effect-risk classification, retry/
    // idempotency law, capability-denial, and the dominator-based approval
    // check over the fully-built graph (covers authored + synthesized nodes).
    super::effects::validate_effects(&b.nodes, &b.edges)?;

    let mut out = Map::new();
    out.insert(
        "_generated".into(),
        json!({
            "by": "zyalc",
            "schema": SCHEMA,
            "do_not_edit_by_hand": true,
            "regenerate": "cargo run -p zyalc -- compile <source.zyal>",
        }),
    );
    out.insert("schema".into(), json!(SCHEMA));
    out.insert("id".into(), json!(id));
    out.insert("job".into(), job);
    out.insert(
        "unroll".into(),
        json!({ "loops": unroll_loops, "generations": unroll_generations }),
    );
    out.insert("nodes".into(), Value::Array(b.nodes));
    out.insert("edges".into(), Value::Array(b.edges));
    if let Some(meta) = generations_meta(root) {
        out.insert("generations_meta".into(), meta);
    }
    // M4-cont: legacy-block → fragment provenance (topology no-op; emitted only
    // when a legacy block is present, so authored-only graphs keep identical IR).
    if let Some(frags) = super::fragments::normalize_legacy_blocks(root) {
        out.insert("fragments".into(), frags);
    }

    // Deterministic IR envelope (law #2): version + content hashes, NO
    // wall-clock. `source_hash` covers the pragma-stripped source, `registry_hash`
    // the embedded node-types registry, `params_hash` the resolved params (empty
    // until params resolution lands in M4). The whole document is then
    // canonicalized so two compiles of the same input are byte-identical.
    let zyal_version = root
        .get("zyal")
        .and_then(Value::as_object)
        .and_then(|z| z.get("version"))
        .and_then(Value::as_str)
        .unwrap_or("1.0")
        .to_string();
    out.insert("ir_version".into(), json!(IR_VERSION));
    out.insert("zyal_version".into(), json!(zyal_version));
    let mut compile = Map::new();
    compile.insert("compiler".into(), json!("zyalc"));
    compile.insert("compiler_version".into(), json!(env!("CARGO_PKG_VERSION")));
    compile.insert("timestamp_policy".into(), json!("omitted_for_determinism"));
    compile.insert(
        "source_hash".into(),
        json!(format!("sha256:{}", super::target::sha256(body))),
    );
    compile.insert(
        "registry_hash".into(),
        json!(super::registry::registry_hash()),
    );
    compile.insert("params_hash".into(), json!(params_hash(root)));
    if !uses.is_empty() {
        let metadata = uses.metadata_value();
        let serialized = serde_json::to_string(&canonicalize(metadata.clone()))
            .unwrap_or_else(|_| "[]".to_string());
        compile.insert(
            "uses_hash".into(),
            json!(format!("sha256:{}", super::target::sha256(&serialized))),
        );
        compile.insert("uses".into(), metadata);
    }
    out.insert("compile".into(), Value::Object(compile));

    Ok(canonicalize(Value::Object(out)))
}

/// Recursively sort every object's keys so the rendered IR is byte-identical
/// regardless of map insertion order or whether serde_json's `preserve_order`
/// feature is unified on elsewhere in the workspace. Array order is preserved
/// (nodes/edges carry semantic/visual order). This is the canonicalization
/// guarantee behind the double-compile determinism test.
fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> =
                map.into_iter().map(|(k, v)| (k, canonicalize(v))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = Map::new();
            for (k, v) in entries {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        other => other,
    }
}

/// Hash of the resolved `params` block (canonicalized). Empty `{}` until param
/// resolution lands; stamped now so the `compile` envelope shape is stable.
fn params_hash(root: &Map<String, Value>) -> String {
    let params = root.get("params").cloned().unwrap_or_else(|| json!({}));
    let canon = canonicalize(params);
    let serialized = serde_json::to_string(&canon).unwrap_or_else(|_| "{}".to_string());
    format!("sha256:{}", super::target::sha256(&serialized))
}

/// Optional typed/taint fields on an emitted edge (F3). All empty for
/// synthesized edges, so untyped graphs keep byte-identical IR.
#[derive(Default)]
struct EdgeTyping {
    from_port: Option<String>,
    to_port: Option<String>,
    payload_type: Option<String>,
    handle_kind: Option<String>,
    taint: Vec<Value>,
}

struct Builder {
    nodes: Vec<Value>,
    ids: std::collections::HashSet<String>,
    edges: Vec<Value>,
    edge_seq: usize,
    /// `backedge` | `unroll` | `auto`
    loops: String,
}

impl Builder {
    fn new(loops: String) -> Self {
        Self {
            nodes: Vec::new(),
            ids: std::collections::HashSet::new(),
            edges: Vec::new(),
            edge_seq: 0,
            loops,
        }
    }

    fn node(&mut self, id: &str, node_type: &str, label: &str, extra: Map<String, Value>) -> bool {
        if !self.ids.insert(id.to_string()) {
            return false; // de-dupe by id
        }
        let mut m = Map::new();
        m.insert("id".into(), json!(id));
        m.insert("node_type".into(), json!(node_type));
        m.insert("label".into(), json!(label));
        for (k, v) in extra {
            m.insert(k, v);
        }
        self.nodes.push(Value::Object(m));
        true
    }

    fn edge(&mut self, src: &str, dst: &str, kind: &str, label: Option<&str>, weight: f64) {
        self.push_edge(src, dst, kind, label, weight, &EdgeTyping::default());
    }

    /// Emit an edge, carrying optional typed/taint fields (F3). Synthesized
    /// edges pass an empty `EdgeTyping`; authored edges may declare ports,
    /// payload type, handle kind, and taint labels — emitted only when present
    /// so untyped graphs keep byte-identical IR.
    fn push_edge(
        &mut self,
        src: &str,
        dst: &str,
        kind: &str,
        label: Option<&str>,
        weight: f64,
        typing: &EdgeTyping,
    ) {
        let loop_back = kind == "loop_back";
        self.edge_seq += 1;
        let mut m = Map::new();
        m.insert("id".into(), json!(format!("e{}", self.edge_seq)));
        m.insert("src".into(), json!(src));
        m.insert("dst".into(), json!(dst));
        m.insert("kind".into(), json!(kind));
        if let Some(l) = label {
            m.insert("label".into(), json!(l));
        }
        m.insert("weight".into(), json!(weight));
        if loop_back {
            m.insert("loop_back".into(), json!(true));
        }
        if let Some(p) = &typing.from_port {
            m.insert("from_port".into(), json!(p));
        }
        if let Some(p) = &typing.to_port {
            m.insert("to_port".into(), json!(p));
        }
        if let Some(t) = &typing.payload_type {
            m.insert("payload_type".into(), json!(t));
        }
        if let Some(h) = &typing.handle_kind {
            m.insert("handle_kind".into(), json!(h));
        }
        if !typing.taint.is_empty() {
            m.insert("taint".into(), Value::Array(typing.taint.clone()));
        }
        self.edges.push(Value::Object(m));
    }

    // ---- authored graph -------------------------------------------------

    // `from_authored` reads the authored block into the builder; it is not a
    // `From`-style constructor, so the `from_*` self-convention lint doesn't apply.
    #[allow(clippy::wrong_self_convention)]
    fn from_authored(&mut self, nodes: &[Value], edges: Option<&Vec<Value>>) {
        for n in nodes {
            let Some(nm) = n.as_object() else { continue };
            self.expand_authored_node(nm);
        }
        if let Some(edges) = edges {
            for e in edges {
                let Some(em) = e.as_object() else { continue };
                let raw_src = em
                    .get("from")
                    .or_else(|| em.get("src"))
                    .and_then(Value::as_str);
                let raw_dst = em
                    .get("to")
                    .or_else(|| em.get("dst"))
                    .and_then(Value::as_str);
                let (Some(raw_src), Some(raw_dst)) = (raw_src, raw_dst) else {
                    continue;
                };
                // `node.port` resolves to (node, port) only when `node` is a real
                // node id (ids never contain `.`), else the whole string is the id.
                let (mut src, from_port_implicit) = self.resolve_ref(raw_src);
                let (mut dst, to_port_implicit) = self.resolve_ref(raw_dst);
                let kind = em.get("kind").and_then(Value::as_str).unwrap_or("feeds");
                let label = em.get("label").and_then(Value::as_str);
                let weight = em.get("weight").and_then(Value::as_f64).unwrap_or(1.0);
                let mut from_port = em
                    .get("from_port")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(from_port_implicit);
                let mut to_port = em
                    .get("to_port")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(to_port_implicit);
                // When a container was expanded into a child named `<node>/<port>`
                // (e.g. a router's `winner` → `<router>/winner`), originate/land the
                // edge on that real inspectable child so data flows from/to the
                // actual node — not the collapsed decision box.
                if let Some(p) = &from_port {
                    let child = format!("{src}/{p}");
                    if self.ids.contains(&child) {
                        src = child;
                        from_port = None;
                    }
                }
                if let Some(p) = &to_port {
                    let child = format!("{dst}/{p}");
                    if self.ids.contains(&child) {
                        dst = child;
                        to_port = None;
                    }
                }
                let typing = EdgeTyping {
                    from_port,
                    to_port,
                    payload_type: em
                        .get("payload_type")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    handle_kind: em
                        .get("handle_kind")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    taint: em
                        .get("taint")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                };
                self.push_edge(&src, &dst, kind, label, weight, &typing);
            }
        }
    }

    /// Split a `node.port` edge endpoint into `(node_id, Some(port))`, but only
    /// when the prefix is a real node id (ids never contain `.`); otherwise the
    /// whole string is the node id with no port.
    fn resolve_ref(&self, raw: &str) -> (String, Option<String>) {
        if let Some((node, port)) = raw.split_once('.') {
            if !port.is_empty() && self.ids.contains(node) {
                return (node.to_string(), Some(port.to_string()));
            }
        }
        (raw.to_string(), None)
    }

    fn expand_authored_node(&mut self, n: &Map<String, Value>) {
        let Some(id) = n.get("id").and_then(Value::as_str) else {
            return;
        };
        let ntype = n.get("type").and_then(Value::as_str).unwrap_or("agent");
        let label = n.get("label").and_then(Value::as_str).unwrap_or(id);
        // Pass every authored key through (cardinality, ports, agent, router,
        // spawn, watcher, kpi, data_feed, artifact, metadata, …) except the
        // structural ones we re-map.
        let mut extra = Map::new();
        for (k, v) in n {
            if !matches!(k.as_str(), "id" | "type" | "label") {
                extra.insert(k.clone(), v.clone());
            }
        }
        self.node(id, ntype, label, extra);
        match ntype {
            "router" => self.expand_router(id, n),
            "spawn" => self.expand_cardinality(id, n, "spawn", "tab", 1),
            "fanout" => self.expand_cardinality(id, n, "fanout", "worker", 0),
            _ => {}
        }
    }

    fn expand_router(&mut self, id: &str, n: &Map<String, Value>) {
        let router = n.get("router").and_then(Value::as_object);
        // `drafts` takes precedence over `backups`; a count of 0 is treated as
        // absent (so `{drafts: 0, fusion: 1}` can't orphan a route_judge).
        let drafts = router
            .and_then(|r| r.get("drafts"))
            .and_then(Value::as_u64)
            .filter(|&d| d > 0);
        let fusion = router
            .and_then(|r| r.get("fusion"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let backups = router
            .and_then(|r| r.get("backups"))
            .and_then(Value::as_u64)
            .filter(|&b| b > 0);

        // No router spec — fall back to a plain cardinality expansion.
        if drafts.is_none() && backups.is_none() {
            self.expand_cardinality(id, n, "fanout", "call", 0);
            return;
        }

        // Compile routing into a VISIBLE topology (M8): the router box is the
        // route_decision; it flows through a budget_gate + health_gate, fans out
        // to N provider_call boxes, then (for fusion) a route_judge, then a
        // route_winner. Nothing is hidden in the runner — every routing decision
        // is an inspectable node.
        let budget = format!("{id}/budget_gate");
        let health = format!("{id}/health_gate");
        self.node(&budget, "budget_gate", "budget", Map::new());
        self.node(&health, "health_gate", "health", Map::new());
        self.edge(id, &budget, "route", None, 1.0);
        self.edge(&budget, &health, "route", None, 1.0);

        let mut call_ids: Vec<String> = Vec::new();
        if let Some(m) = drafts {
            for i in 0..m {
                let cid = format!("{id}/call-{i}");
                self.node(&cid, "provider_call", &format!("provider {i}"), Map::new());
                self.edge(&health, &cid, "fanout", None, 1.0);
                call_ids.push(cid);
            }
        } else if let Some(nbk) = backups {
            let primary = format!("{id}/call-0");
            self.node(&primary, "provider_call", "primary", Map::new());
            self.edge(&health, &primary, "fanout", None, 1.0);
            call_ids.push(primary);
            for i in 0..nbk {
                let cid = format!("{id}/call-{}", i + 1);
                self.node(&cid, "provider_call", &format!("backup {i}"), Map::new());
                self.edge(&health, &cid, "backup", None, 1.0);
                call_ids.push(cid);
            }
        }

        let winner = format!("{id}/winner");
        if fusion >= 1 && drafts.is_some() {
            // fusion: a route_judge reduces the provider calls, then a winner.
            let judge = format!("{id}/judge");
            self.node(&judge, "route_judge", "route judge", Map::new());
            for c in &call_ids {
                self.edge(c, &judge, "vote", None, 1.0);
            }
            self.node(
                &winner,
                "route_winner",
                "winner",
                obj(&[("winner", json!(true))]),
            );
            self.edge(&judge, &winner, "winner", None, 1.0);
        } else {
            // no fusion / backup mode: the first call wins, the rest escalate.
            self.node(
                &winner,
                "route_winner",
                "winner",
                obj(&[("winner", json!(true))]),
            );
            if let Some(first) = call_ids.first() {
                self.edge(first, &winner, "winner", None, 1.0);
            }
            for c in call_ids.iter().skip(1) {
                self.edge(c, &winner, "escalation", None, 0.6);
            }
        }
    }

    /// Expand a container node into N child agent boxes. `start` is the first
    /// child index (jailgun tabs are 1-based; workers are 0-based).
    fn expand_cardinality(
        &mut self,
        id: &str,
        n: &Map<String, Value>,
        edge_kind: &str,
        child: &str,
        start: u64,
    ) {
        let card = n.get("cardinality").or_else(|| {
            n.get("spawn")
                .and_then(Value::as_object)
                .and_then(|s| s.get("cardinality"))
        });
        let (count, resolved) = card.map(count_of).unwrap_or((None, false));
        match count {
            Some(k) if k > 0 => {
                for i in start..(start + k) {
                    let cid = format!("{id}/{child}-{i}");
                    let extra = if resolved {
                        Map::new()
                    } else {
                        obj(&[("cardinality_resolved", json!("runtime_estimate"))])
                    };
                    self.node(&cid, "agent", &format!("{child} {i}"), extra);
                    self.edge(id, &cid, edge_kind, None, 1.0);
                }
            }
            _ => {
                // Cardinality only known at runtime (F1b): emit a `group.dynamic`
                // placeholder carrying the formal cardinality + `runtime_state:
                // pending`. A runtime `GraphPatched::ExpandDynamicGroup` resolves
                // it into concrete instances later.
                let cid = format!("{id}/{child}-runtime");
                let extra = match card {
                    Some(c) => obj(&[(
                        "cardinality",
                        super::cardinality::parse_cardinality(c).placeholder_value(),
                    )]),
                    None => obj(&[("cardinality_resolved", json!("runtime"))]),
                };
                self.node(&cid, "group.dynamic", &format!("{child} ×N"), extra);
                self.edge(id, &cid, edge_kind, Some("×N at runtime"), 1.0);
            }
        }
    }

    // ---- synthesis from policy blocks -----------------------------------

    fn synthesize(&mut self, root: &Map<String, Value>) {
        let agents = root.get("agents").and_then(Value::as_object);
        let supervisor_agent = agents
            .and_then(|a| a.get("supervisor"))
            .and_then(Value::as_object)
            .and_then(|s| s.get("agent"))
            .and_then(Value::as_str)
            .unwrap_or("supervisor");
        let has_supervisor = self.node(
            "supervisor",
            "supervisor",
            supervisor_agent,
            obj(&[("agent", json!(supervisor_agent))]),
        );

        // Worker pools: agents.workers[] { id, count, agent }
        if let Some(workers) = agents
            .and_then(|a| a.get("workers"))
            .and_then(Value::as_array)
        {
            for w in workers {
                let Some(wm) = w.as_object() else { continue };
                let wid = wm.get("id").and_then(Value::as_str).unwrap_or("workers");
                let count = wm.get("count").and_then(Value::as_u64).unwrap_or(1).max(1);
                let pool = format!("worker-pool/{wid}");
                self.node(&pool, "fanout", &format!("{wid} ×{count}"), Map::new());
                if has_supervisor {
                    self.edge("supervisor", &pool, "dispatches", None, 1.0);
                }
                let agent = wm.get("agent").and_then(Value::as_str).unwrap_or("build");
                for i in 0..count {
                    let cid = format!("{pool}/w-{i}");
                    self.node(
                        &cid,
                        "agent",
                        &format!("{agent} {i}"),
                        obj(&[("agent", json!(agent))]),
                    );
                    self.edge(&pool, &cid, "fanout", None, 1.0);
                }
            }
        }

        // fan_out { split { items|value }, worker { agent }, reduce }
        if let Some(fo) = root.get("fan_out").and_then(Value::as_object) {
            let split = fo.get("split").and_then(Value::as_object);
            let count = split
                .and_then(|s| s.get("items"))
                .and_then(Value::as_array)
                .map(|a| a.len() as u64)
                .or_else(|| split.and_then(|s| s.get("value")).and_then(Value::as_u64));
            let agent = fo
                .get("worker")
                .and_then(Value::as_object)
                .and_then(|w| w.get("agent"))
                .and_then(Value::as_str)
                .unwrap_or("worker");
            self.node("fan_out", "fanout", "fan-out", Map::new());
            if has_supervisor {
                self.edge("supervisor", "fan_out", "dispatches", None, 1.0);
            }
            let n = count.unwrap_or(0);
            let mut child_ids = Vec::new();
            if n > 0 {
                for i in 0..n {
                    let cid = format!("fan_out/{agent}-{i}");
                    self.node(
                        &cid,
                        "agent",
                        &format!("{agent} {i}"),
                        obj(&[("agent", json!(agent))]),
                    );
                    self.edge("fan_out", &cid, "fanout", None, 1.0);
                    child_ids.push(cid);
                }
            } else {
                let cid = format!("fan_out/{agent}-runtime");
                self.node(
                    &cid,
                    "agent",
                    &format!("{agent} ×N"),
                    obj(&[("cardinality_resolved", json!("runtime"))]),
                );
                self.edge("fan_out", &cid, "fanout", Some("×N at runtime"), 1.0);
                child_ids.push(cid);
            }
            if fo.contains_key("reduce") {
                self.node("fan_out/reduce", "fanin", "reduce", Map::new());
                for c in &child_ids {
                    self.edge(c, "fan_out/reduce", "reduced_into", None, 1.0);
                }
            }
        }

        // dispatch { classifier, lanes[] { id, dispatch_to } }
        if let Some(dispatch) = root.get("dispatch").and_then(Value::as_object) {
            self.node("dispatch", "dispatch_classifier", "classify", Map::new());
            if has_supervisor {
                self.edge("supervisor", "dispatch", "dispatches", None, 1.0);
            }
            if let Some(lanes) = dispatch.get("lanes").and_then(Value::as_array) {
                for lane in lanes {
                    let Some(lm) = lane.as_object() else { continue };
                    let lid = lm.get("id").and_then(Value::as_str).unwrap_or("lane");
                    let to = lm.get("dispatch_to").and_then(Value::as_str).unwrap_or(lid);
                    let nid = format!("dispatch/{lid}");
                    self.node(&nid, "route", lid, obj(&[("dispatch_to", json!(to))]));
                    self.edge("dispatch", &nid, "route", Some(lid), 0.8);
                }
            }
        }

        // hero_judge lowers into the canonical tournament kernel (M10).
        self.synthesize_tournament(root, has_supervisor);

        // Nothing matched — at least emit the job as a single node.
        if self.nodes.is_empty() {
            let label = root
                .get("job")
                .and_then(Value::as_object)
                .and_then(|j| j.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("workflow");
            self.node("job", "agent", label, Map::new());
        }
    }

    /// Lower a `hero_judge` (or legacy `experiments`) block into the canonical
    /// tournament chain (M10): `generation_gate → reasoning_lane[] → judge_panel
    /// → verifier_panel → refute_lane[] → promotion_gate`. Generations are a
    /// single body + one curved loop-back edge by default (constant node count
    /// for 50-generation runs) — never a visual axis.
    fn synthesize_tournament(&mut self, root: &Map<String, Value>, has_supervisor: bool) {
        let Some(hj) = root.get("hero_judge").and_then(Value::as_object) else {
            return;
        };
        let generations = hj
            .get("generations")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1);
        let pop = hj.get("population").and_then(Value::as_object);
        let count = |key: &str, default: u64| {
            pop.and_then(|p| p.get(key))
                .and_then(Value::as_u64)
                .unwrap_or(default)
                .max(1)
        };
        let hero_lanes = count("hero_lanes", 3);
        let judge_lanes = count("judge_lanes", 1);
        let refute_lanes = count("refute_lanes", 1);

        let do_unroll =
            self.loops == "unroll" || (self.loops == "auto" && generations <= AUTO_UNROLL_MAX);
        let bodies = if do_unroll { generations } else { 1 };

        let mut first_entry: Option<String> = None;
        let mut last_exit: Option<String> = None;
        for g in 0..bodies {
            let sfx = if do_unroll {
                format!("/gen-{g}")
            } else {
                String::new()
            };
            let research = format!("research{sfx}");
            self.node(&research, "web_search", "research", Map::new());
            if first_entry.is_none() {
                first_entry = Some(research.clone());
            }
            if has_supervisor && g == 0 {
                self.edge("supervisor", &research, "dispatches", None, 1.0);
            }
            if let Some(prev) = &last_exit {
                self.edge(prev, &research, "feeds", Some("next generation"), 0.5);
            }

            let gate = format!("generation_gate{sfx}");
            self.node(&gate, "generation_gate", "generation", Map::new());
            self.edge(&research, &gate, "feeds", None, 1.0);

            // judge panel reduces the reasoning lanes (blind cross-provider judging
            // is enforced at runtime — see `jekko_runner::tournament::enforce_blind`).
            let judge = format!("judge_panel{sfx}");
            self.node(
                &judge,
                "judge_panel",
                &format!("judge ×{judge_lanes}"),
                Map::new(),
            );
            for i in 0..hero_lanes {
                let lane = format!("reasoning_lane{sfx}/lane-{i}");
                self.node(&lane, "reasoning_lane", &format!("lane {i}"), Map::new());
                self.edge(&gate, &lane, "fanout", None, 1.0);
                self.edge(&lane, &judge, "vote", None, 1.0);
            }

            let verifier = format!("verifier_panel{sfx}");
            self.node(&verifier, "verifier_panel", "verifier", Map::new());
            self.edge(&judge, &verifier, "verified_by", None, 1.0);

            let promotion = format!("promotion_gate{sfx}");
            self.node(
                &promotion,
                "promotion_gate",
                "promotion",
                obj(&[("winner", json!(true))]),
            );
            for j in 0..refute_lanes {
                let refute = format!("refute_lane{sfx}/lane-{j}");
                self.node(&refute, "refute_lane", &format!("refute {j}"), Map::new());
                self.edge(&verifier, &refute, "critique", None, 1.0);
                self.edge(&refute, &promotion, "reduced_into", None, 1.0);
            }
            last_exit = Some(promotion);
        }

        // Backedge mode (default / large generation count): one body + a
        // labeled loop-back edge instead of unrolling.
        if !do_unroll && generations > 1 {
            if let (Some(entry), Some(exit)) = (&first_entry, &last_exit) {
                self.edge(
                    exit,
                    entry,
                    "loop_back",
                    Some(&format!("repeat ×{generations}")),
                    0.3,
                );
            }
        }
    }

    // ---- pattern compiler (M7) ------------------------------------------

    /// Lower parsed `patterns:` specs into concrete FlowGraph nodes/edges, each
    /// tagged with `pattern` provenance (`{id, template_node}`).
    fn emit_patterns(&mut self, specs: &[super::patterns::PatternSpec]) {
        use super::patterns::PatternSpec;
        for spec in specs {
            match spec {
                PatternSpec::ScatterGather {
                    id,
                    count,
                    max,
                    branch_agent,
                    gather_agent,
                    reduce,
                } => {
                    let split = format!("pattern/{id}/split");
                    self.node(
                        &split,
                        "fanout",
                        &format!("{id} scatter"),
                        prov(id, "split"),
                    );
                    let gather = format!("pattern/{id}/gather");
                    match count {
                        Some(n) if *n > 0 => {
                            for i in 0..*n {
                                let bid = format!("pattern/{id}/branch-{i}");
                                let mut extra = prov(id, "branch");
                                extra.insert("agent".into(), json!(branch_agent));
                                extra.insert("parent".into(), json!(split));
                                self.node(&bid, "agent", &format!("{branch_agent} {i}"), extra);
                                self.edge(&split, &bid, "fanout", None, 1.0);
                                self.edge(&bid, &gather, "reduced_into", None, 1.0);
                            }
                        }
                        _ => {
                            // Runtime-resolved branch count: a bounded dynamic group.
                            let bid = format!("pattern/{id}/branch-runtime");
                            let mut extra = prov(id, "branch");
                            extra.insert("parent".into(), json!(split));
                            extra.insert(
                                "cardinality".into(),
                                json!({ "mode": "discovered", "max": max, "runtime_state": "pending" }),
                            );
                            self.node(&bid, "group.dynamic", &format!("{branch_agent} ×N"), extra);
                            self.edge(&split, &bid, "fanout", Some("×N at runtime"), 1.0);
                            self.edge(&bid, &gather, "reduced_into", None, 1.0);
                        }
                    }
                    let mut gextra = prov(id, "gather");
                    gextra.insert("agent".into(), json!(gather_agent));
                    gextra.insert("reduce".into(), json!(reduce));
                    self.node(&gather, "fanin", &format!("gather ({reduce})"), gextra);
                }
                PatternSpec::LoopUntilDry {
                    id,
                    body_agent,
                    max_rounds,
                    marginal_threshold,
                } => {
                    let body = format!("pattern/{id}/body");
                    let mut bextra = prov(id, "body");
                    bextra.insert("agent".into(), json!(body_agent));
                    self.node(&body, "agent", &format!("{id} body"), bextra);
                    let dry = format!("pattern/{id}/dry_certificate");
                    let mut dextra = prov(id, "dry_certificate");
                    if let Some(m) = marginal_threshold {
                        dextra.insert("marginal_utility_below".into(), json!(m));
                    }
                    if let Some(r) = max_rounds {
                        dextra.insert("max_rounds".into(), json!(r));
                    }
                    self.node(&dry, "dry_certificate", "dry?", dextra);
                    self.edge(&body, &dry, "feeds", None, 1.0);
                    let label = match marginal_threshold {
                        Some(m) => format!("until dry (≤{m})"),
                        None => "until dry".to_string(),
                    };
                    self.edge(&dry, &body, "loop_back", Some(&label), 0.3);
                }
            }
        }
    }

    // ---- capability blocks (watchers / kpi / data_feeds / artifacts) ----

    fn capability_blocks(&mut self, root: &Map<String, Value>) {
        self.block_nodes(root.get("watchers"), "watcher", "watcher", "watcher");
        if let Some(kpi) = root.get("kpi").and_then(Value::as_object) {
            if let Some(global) = kpi.get("global") {
                self.block_nodes(Some(global), "kpi", "kpi", "kpi");
            } else {
                self.block_nodes(root.get("kpi"), "kpi", "kpi", "kpi");
            }
        }
        self.block_nodes(root.get("data_feeds"), "data_feed", "feed", "data_feed");
        self.block_nodes(root.get("artifacts"), "artifact", "artifact", "artifact");
    }

    /// Turn an array-or-map capability block into side-channel nodes. Each entry
    /// carries its full spec under `payload_key` so the renderer can show
    /// equations / values / plots / previews.
    fn block_nodes(
        &mut self,
        block: Option<&Value>,
        node_type: &str,
        prefix: &str,
        payload_key: &str,
    ) {
        let Some(block) = block else { return };
        let entries: Vec<(String, Value)> = match block {
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let id = v
                        .as_object()
                        .and_then(|o| o.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("{prefix}-{i}"));
                    (id, v.clone())
                })
                .collect(),
            Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            _ => return,
        };
        for (name, spec) in entries {
            let nid = format!("{prefix}/{name}");
            let label = spec
                .as_object()
                .and_then(|o| o.get("name").or_else(|| o.get("label")))
                .and_then(Value::as_str)
                .unwrap_or(&name)
                .to_string();
            self.node(&nid, node_type, &label, obj(&[(payload_key, spec)]));
        }
    }
}

/// Resolve a cardinality spec to `(count, resolved_exactly)`. Delegates to the
/// single cardinality parser so the new `mode` key and the legacy `kind` key
/// behave identically (F1b).
fn count_of(card: &Value) -> (Option<u64>, bool) {
    let spec = super::cardinality::parse_cardinality(card);
    (spec.value, spec.resolved())
}

fn generations_meta(root: &Map<String, Value>) -> Option<Value> {
    let hj = root.get("hero_judge").and_then(Value::as_object)?;
    let total = hj.get("generations").and_then(Value::as_u64)?;
    let window_end = total.saturating_sub(1);
    Some(json!({ "total": total, "window": [0, window_end] }))
}

fn obj(pairs: &[(&str, Value)]) -> Map<String, Value> {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    m
}

/// Provenance stamp for a pattern-lowered node (`pattern/<id>/<template_node>`).
fn prov(id: &str, template_node: &str) -> Map<String, Value> {
    obj(&[(
        "pattern",
        json!({ "id": id, "template_node": template_node }),
    )])
}
