use std::fs;
use std::path::Path;

use super::emit::{
    emit_flowgraph, emit_flowgraph_with_source, emit_superworkflow, emit_toml, strip_pragmas,
};
use super::target::source_reference;
use super::*;

#[test]
fn round_trip_strips_pragmas() {
    let raw = "# zyal: declarative target=toml schema=test@1\nschema_version: \"1.0.0\"\nlanes:\n  - name: a\n    command_id: x.a\n    cost: 1\n";
    let stripped = strip_pragmas(raw);
    assert!(!stripped.contains("# zyal:"));
    assert!(stripped.contains("schema_version"));
}

#[test]
fn strip_pragmas_uses_zyal_envelope_body() {
    let raw = "# preamble: comment\n# zyal: declarative target=superworkflow schema=zyal/superworkflow@1\n<<<ZYAL v1:daemon id=smoke>>>\nversion: v1\nintent: daemon\n<<<END_ZYAL id=smoke>>>\n";
    let stripped = strip_pragmas(raw);
    assert!(!stripped.contains("preamble: comment"));
    assert!(!stripped.contains("<<<ZYAL"));
    assert!(stripped.contains("version: v1"));
}

#[test]
fn toml_emit_basic() {
    let raw = "# zyal: declarative target=toml schema=t@1\nschema_version: \"1.0.0\"\nlanes:\n  - name: a\n    cost: 1\n";
    let out = emit_toml(raw).expect("emit");
    assert!(out.contains("schema_version"));
    assert!(out.contains("[[lane]]"));
    assert!(out.contains("name = \"a\""));
}

#[test]
fn toml_emit_accepts_split_runbook_and_lane_tail() {
    let raw = "# zyal: declarative target=toml schema=jankurai/sandbox-lanes@1\n\
         <<<ZYAL v1:daemon id=sandbox-lanes-template>>>\n\
         version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: sandbox-lanes-template\n\
         job:\n  name: sandbox lanes\n  objective: keep sandbox lanes in sync\n\
         stop:\n  all:\n    - git_clean:\n        allow_untracked: false\n\
         <<<END_ZYAL id=sandbox-lanes-template>>>\n";
    let raw = format!(
        "{raw}schema_version: \"1.0.0\"\nsandbox_root: \"~/.local/share/agent-sandboxes\"\nlanes:\n  - name: a\n    cost: 1\n"
    );
    let out = emit_toml(&raw).expect("emit split runbook and lane tail");
    assert!(out.contains("schema_version"));
    assert!(out.contains("[[lane]]"));
    assert!(out.contains("name = \"a\""));
    assert!(!out.contains("job ="));
    assert!(!out.contains("stop ="));
    assert!(!out.contains("dispatch ="));
}

#[test]
fn toml_emit_accepts_top_level_lanes_block() {
    let raw = "# zyal: declarative target=toml schema=jankurai/sandbox-lanes@1\n\
         <<<ZYAL v1:daemon id=sandbox-lanes-template>>>\n\
         version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: sandbox-lanes-template\n\
         job:\n  name: sandbox lanes\n  objective: keep sandbox lanes in sync\n\
         stop:\n  all:\n    - git_clean:\n        allow_untracked: false\n\
         sandbox:\n  schema_version: \"1.0.0\"\n  sandbox_root: \"~/.local/share/agent-sandboxes\"\n\
         lanes:\n    - name: a\n      cost: 1\n\
         <<<END_ZYAL id=sandbox-lanes-template>>>\n";
    let out = emit_toml(raw).expect("emit top-level sandbox lanes");
    assert!(out.contains("schema_version"));
    assert!(out.contains("[[lane]]"));
    assert!(out.contains("name = \"a\""));
    assert!(!out.contains("job ="));
    assert!(!out.contains("stop ="));
}

#[test]
fn idempotent_emit() {
    let raw = "# zyal: declarative target=toml schema=t@1\nschema_version: \"1.0.0\"\nlanes:\n  - name: a\n    cost: 1\n";
    let a = emit_toml(raw).unwrap();
    let b = emit_toml(raw).unwrap();
    assert_eq!(a, b, "compile must be idempotent");
}

#[test]
fn runbook_profiles_validate_without_emitting_legacy_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("smoke.zyal");
    fs::write(
        &source,
        "<<<ZYAL v1:daemon id=smoke>>>\njob:\n  name: smoke\n<<<END_ZYAL id=smoke>>>\n",
    )
    .unwrap();

    let outcome = compile_one(&source, None, true).unwrap();
    assert!(matches!(outcome, Outcome::Unchanged(path) if path == source));
    assert!(
        !source.with_extension("yml").exists(),
        "runbook validation must not emit retired .zyal.yml artifacts"
    );
}

#[test]
fn source_reference_preserves_canonical_subdirectory() {
    assert_eq!(
        source_reference(Path::new("./agent/zyal/sandbox-lanes.zyal")),
        "agent/zyal/sandbox-lanes.zyal"
    );
}

// --- SuperWorkflow validation + emission ----------------------------------

/// Build a minimal valid SuperWorkflow manifest with `n` independent phases
/// (`p0`, `p1`, ...). Used by the validation tests below to exercise edge
/// cases without repeating the entire pragma/header preamble.
fn superworkflow_with_phases(n: usize) -> String {
    let mut raw = String::from(
        "# zyal: declarative target=superworkflow schema=zyal/superworkflow@1\n\
         version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: smoke\n\
         job:\n  name: smoke\n  objective: smoke\n\
         superworkflow:\n",
    );
    raw.push_str(&format!("  stage_count: {n}\n  phases:\n"));
    for idx in 0..n {
        raw.push_str(&format!(
            "    - id: p{idx}\n      name: p{idx}\n      objective: p{idx}\n      exit:\n        required_artifacts: [target/p{idx}.json]\n        gates:\n          - kind: artifact_exists\n"
        ));
    }
    raw
}

fn workflow_state_machine_with_linear_states(n: usize) -> String {
    let mut raw = String::from("workflow:\n  type: state_machine\n  initial: p0\n  states:\n");
    for idx in 0..n {
        raw.push_str(&format!(
            "    p{idx}:\n      agent: build\n      writes: scratch_only\n      produces: [target/p{idx}.json]\n"
        ));
        if idx + 1 < n {
            raw.push_str(&format!(
                "      transitions:\n        - to: p{}\n          when: {{ evidence_exists: target/p{idx}.json }}\n",
                idx + 1
            ));
        } else {
            raw.push_str("      terminal: true\n");
        }
    }
    raw
}

#[test]
fn superworkflow_emit_requires_nine_to_twelve_phases() {
    let raw = superworkflow_with_phases(9);
    let out = emit_superworkflow(&raw).expect("9-phase superworkflow json");
    assert!(out.contains("\"superworkflow\""));
    assert!(out.contains("\"phases\""));

    let raw = superworkflow_with_phases(12);
    emit_superworkflow(&raw).expect("12-phase superworkflow json");
}

#[test]
fn superworkflow_emit_accepts_workflow_root_shape() {
    let workflow = workflow_state_machine_with_linear_states(9);
    let raw = format!(
        "# zyal: declarative target=superworkflow schema=zyal/superworkflow@1\n\
         version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: smoke\n\
         job:\n  name: smoke\n  objective: smoke\n{workflow}"
    );
    let out = emit_superworkflow(&raw).expect("workflow-root superworkflow json");
    assert!(out.contains("\"workflow\""));
    assert!(out.contains("\"superworkflow\""));
    assert!(out.contains("\"phases\""));
}

#[test]
fn superworkflow_emit_accepts_job_nested_shape() {
    let mut raw = String::from(
        "# zyal: declarative target=superworkflow schema=zyal/superworkflow@1\n\
         version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: smoke\n\
         job:\n  name: smoke\n  objective: smoke\n  superworkflow:\n",
    );
    raw.push_str("    stage_count: 9\n    phases:\n");
    for idx in 0..9 {
        raw.push_str(&format!(
            "      - id: p{idx}\n        name: p{idx}\n        objective: p{idx}\n        exit:\n          required_artifacts: [target/p{idx}.json]\n          gates:\n            - kind: artifact_exists\n"
        ));
    }
    let out = emit_superworkflow(&raw).expect("nested superworkflow json");
    assert!(out.contains("\"job\""));
    assert!(out.contains("\"superworkflow\""));
    assert!(out.contains("\"phases\""));
}

#[test]
fn superworkflow_rejects_too_few_phases() {
    let raw = superworkflow_with_phases(1);
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(format!("{err}").contains("requires 9-12 phases"));
}

#[test]
fn superworkflow_rejects_duplicate_phase_ids() {
    // Rewrite the `p1` block so it claims `id: p0`, colliding with the first.
    let raw = superworkflow_with_phases(9).replace("- id: p1\n", "- id: p0\n");
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("duplicate superworkflow phase id"),
        "expected duplicate id error, got: {err}"
    );
}

#[test]
fn superworkflow_rejects_self_dependency() {
    let mut raw = superworkflow_with_phases(9);
    raw = raw.replacen(
        "- id: p0\n      name: p0\n      objective: p0\n      exit:\n",
        "- id: p0\n      name: p0\n      objective: p0\n      depends_on: [p0]\n      exit:\n",
        1,
    );
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("depends on itself"),
        "expected self-dependency error, got: {err}"
    );
}

#[test]
fn superworkflow_rejects_unknown_dependency() {
    let mut raw = superworkflow_with_phases(9);
    raw = raw.replacen(
        "- id: p0\n      name: p0\n      objective: p0\n      exit:\n",
        "- id: p0\n      name: p0\n      objective: p0\n      depends_on: [nope]\n      exit:\n",
        1,
    );
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("unknown phase"),
        "expected unknown-dependency error, got: {err}"
    );
}

#[test]
fn superworkflow_rejects_non_sequence_dependency_list() {
    let mut raw = superworkflow_with_phases(9);
    raw = raw.replacen(
        "- id: p0\n      name: p0\n      objective: p0\n      exit:\n",
        "- id: p0\n      name: p0\n      objective: p0\n      depends_on: p1\n      exit:\n",
        1,
    );
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("dependency list must be a sequence"),
        "expected sequence type error, got: {err}"
    );
}

#[test]
fn superworkflow_rejects_cycle() {
    // Wire `p0` -> `p1` and `p1` -> `p0` so the dependency graph is a 2-cycle.
    let mut raw = superworkflow_with_phases(9);
    raw = raw.replacen(
        "- id: p0\n      name: p0\n      objective: p0\n      exit:\n",
        "- id: p0\n      name: p0\n      objective: p0\n      depends_on: [p1]\n      exit:\n",
        1,
    );
    raw = raw.replacen(
        "- id: p1\n      name: p1\n      objective: p1\n      exit:\n",
        "- id: p1\n      name: p1\n      objective: p1\n      depends_on: [p0]\n      exit:\n",
        1,
    );
    let err = emit_superworkflow(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("cycle"),
        "expected cycle error, got: {err}"
    );
}

// ---- FlowGraph IR -------------------------------------------------------

fn flowgraph_json(raw: &str) -> serde_json::Value {
    let rendered = emit_flowgraph(raw).expect("emit flowgraph");
    serde_json::from_str(&rendered).expect("flowgraph output is valid JSON")
}

fn node_ids_of_type<'a>(ir: &'a serde_json::Value, ty: &str) -> Vec<&'a str> {
    ir["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["node_type"] == ty)
        .filter_map(|n| n["id"].as_str())
        .collect()
}

const FLOWGRAPH_HEADER: &str = "# zyal: declarative target=flowgraph schema=zyal/flowgraph@1\n\
     version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: fg-test\n\
     job:\n  name: fg test\n  objective: exercise flowgraph emit\n";

#[test]
fn flowgraph_authored_expands_router_spawn_and_fanout() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  unroll: {{ loops: backedge }}\n  nodes:\n\
         \x20   - {{ id: sup, type: supervisor, label: Planner }}\n\
         \x20   - {{ id: route, type: router, label: Route, router: {{ drafts: 3, fusion: 1 }} }}\n\
         \x20   - {{ id: tabs, type: spawn, label: Tabs, spawn: {{ cardinality: {{ kind: config, value: 5 }} }} }}\n\
         \x20   - {{ id: feed, type: data_feed, label: Feed, data_feed: {{ kind: stock_ticker }} }}\n\
         \x20   - {{ id: kpi1, type: kpi, label: Cost }}\n\
         \x20   - {{ id: watch1, type: watcher, label: Conv, watcher: {{ expr: \"a-b\" }} }}\n\
         \x20   - {{ id: paper, type: artifact, label: Paper }}\n\
         \x20 edges:\n\
         \x20   - {{ from: sup, to: route, kind: dispatches }}\n\
         \x20   - {{ from: route, to: tabs, kind: feeds }}\n\
         \x20   - {{ from: paper, to: sup, kind: loop_back, label: again }}\n"
    );
    let ir = flowgraph_json(&raw);
    assert_eq!(ir["schema"], "zyal/flowgraph@1");
    // router -> visible chain: 3 provider_call boxes + a route_winner; spawn -> 5 tabs (1-based)
    let agents = node_ids_of_type(&ir, "agent");
    assert_eq!(
        node_ids_of_type(&ir, "provider_call")
            .iter()
            .filter(|id| id.starts_with("route/call-"))
            .count(),
        3
    );
    assert_eq!(
        agents
            .iter()
            .filter(|id| id.starts_with("tabs/tab-"))
            .count(),
        5
    );
    assert!(agents.contains(&"tabs/tab-1") && agents.contains(&"tabs/tab-5"));
    // the visible routing chain + capability node kinds are all present
    for ty in [
        "data_feed",
        "kpi",
        "watcher",
        "artifact",
        "supervisor",
        "router",
        "spawn",
        "budget_gate",
        "health_gate",
        "provider_call",
        "route_judge",
        "route_winner",
    ] {
        assert!(
            !node_ids_of_type(&ir, ty).is_empty(),
            "missing node type {ty}"
        );
    }
    // the route_winner box is the winner
    let winner = ir["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "route/winner")
        .unwrap();
    assert_eq!(winner["winner"], true);
    // the loop-back edge survives as a back-edge, not a generation node
    let loop_back = ir["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["loop_back"] == true && e["src"] == "paper" && e["dst"] == "sup");
    assert!(loop_back, "expected a loop_back edge paper -> sup");
}

#[test]
fn flowgraph_synthesizes_from_hero_judge_with_backedge() {
    // No authored graph: synthesize from policy blocks. Default loops:backedge
    // means a 3-generation tournament emits ONE body + a loop-back edge.
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         agents:\n  supervisor:\n    agent: plan\n\
         hero_judge:\n  generations: 3\n  population:\n    hero_lanes: 4\n    judge_lanes: 2\n"
    );
    let ir = flowgraph_json(&raw);
    // canonical tournament chain: 4 reasoning lanes + the gate/judge/verifier/refute/promotion nodes
    assert_eq!(
        node_ids_of_type(&ir, "reasoning_lane")
            .iter()
            .filter(|id| id.starts_with("reasoning_lane/lane-"))
            .count(),
        4
    );
    for ty in [
        "generation_gate",
        "judge_panel",
        "verifier_panel",
        "refute_lane",
        "promotion_gate",
    ] {
        assert!(
            !node_ids_of_type(&ir, ty).is_empty(),
            "missing tournament node kind {ty}"
        );
    }
    assert!(!node_ids_of_type(&ir, "supervisor").is_empty());
    // backedge (not unroll): exactly one research node + one loop_back edge (constant
    // node count regardless of generation count)
    assert_eq!(node_ids_of_type(&ir, "web_search").len(), 1);
    let back = ir["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["loop_back"] == true)
        .count();
    assert_eq!(
        back, 1,
        "expected exactly one loop-back edge in backedge mode"
    );
    assert_eq!(ir["generations_meta"]["total"], 3);
}

#[test]
fn flowgraph_unroll_materializes_generations() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  unroll: {{ loops: unroll }}\n\
         agents:\n  supervisor:\n    agent: plan\n\
         hero_judge:\n  generations: 3\n  population:\n    hero_lanes: 2\n    judge_lanes: 1\n"
    );
    let ir = flowgraph_json(&raw);
    // unroll: 3 research nodes (one per generation), no loop-back edge
    assert_eq!(node_ids_of_type(&ir, "web_search").len(), 3);
    let back = ir["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["loop_back"] == true)
        .count();
    assert_eq!(back, 0, "unroll mode must not emit a loop-back edge");
}

#[test]
fn flowgraph_rejects_missing_job_objective() {
    let raw = "# zyal: declarative target=flowgraph schema=zyal/flowgraph@1\n\
         version: v1\nintent: daemon\nconfirm: RUN_FOREVER\nid: fg-test\n\
         job:\n  name: fg test\n";
    let err = emit_flowgraph(raw).unwrap_err();
    assert!(format!("{err}").contains("objective"), "got: {err}");
}

#[test]
fn flagship_openqg_example_compiles_to_expected_ir() {
    // The tracked flagship example is the golden source: it must compile to an IR
    // with the full fan-out / jailgun / tournament / watcher / kpi / feed / artifact
    // shape and a loop-back edge (no generation axis).
    let path = format!(
        "{}/../../docs/ZYAL/examples/35-flowgraph-openqg-foundry.zyal",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let ir = flowgraph_json(&raw);
    assert_eq!(ir["id"], "openqg-foundry");

    let agents = node_ids_of_type(&ir, "agent");
    // jnoccio fusion route -> a VISIBLE chain: 4 provider_call boxes + a route_winner
    assert_eq!(
        node_ids_of_type(&ir, "provider_call")
            .iter()
            .filter(|id| id.starts_with("route_fusion/call-"))
            .count(),
        4
    );
    assert!(ir["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == "route_fusion/winner" && n["winner"] == true));
    // jailgun spawn -> 5 tab boxes
    assert_eq!(
        agents
            .iter()
            .filter(|id| id.starts_with("jailgun_pool/tab-"))
            .count(),
        5
    );
    // hero fan-out -> 3 candidate boxes
    assert_eq!(
        agents
            .iter()
            .filter(|id| id.starts_with("hero/worker-"))
            .count(),
        3
    );
    // every first-class node kind renders (incl. the visible routing chain)
    for ty in [
        "supervisor",
        "router",
        "spawn",
        "fanin",
        "data_feed",
        "kpi",
        "watcher",
        "artifact",
        "budget_gate",
        "health_gate",
        "provider_call",
        "route_judge",
        "route_winner",
    ] {
        assert!(
            !node_ids_of_type(&ir, ty).is_empty(),
            "missing node kind {ty}"
        );
    }
    // iteration is a single loop-back edge, not a generation node
    let loop_back =
        ir["edges"].as_array().unwrap().iter().any(|e| {
            e["loop_back"] == true && e["src"] == "report_pdf" && e["dst"] == "supervisor"
        });
    assert!(
        loop_back,
        "expected loop_back edge report_pdf -> supervisor"
    );
}

#[test]
fn flowgraph_rejects_duplicate_node_ids() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: dup, type: agent }}\n\
         \x20   - {{ id: dup, type: agent }}\n"
    );
    let err = emit_flowgraph(&raw).unwrap_err();
    assert!(format!("{err}").contains("duplicate"), "got: {err}");
}

#[test]
fn flowgraph_ir_is_byte_deterministic() {
    // Law #2: compiling the same source twice yields byte-identical IR. This is
    // the keystone determinism guarantee every later milestone builds on.
    let path = format!(
        "{}/../../docs/ZYAL/examples/35-flowgraph-openqg-foundry.zyal",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let first = emit_flowgraph(&raw).expect("emit flowgraph #1");
    let second = emit_flowgraph(&raw).expect("emit flowgraph #2");
    assert_eq!(
        first, second,
        "FlowGraph IR must be byte-identical on recompile"
    );
    // And no wall-clock leaked into the envelope.
    assert!(
        !first.contains("\"timestamp\"") && !first.contains("\"generated_at\""),
        "IR must not embed a wall-clock"
    );
}

#[test]
fn flowgraph_ir_stamps_deterministic_compile_envelope() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: a, type: agent, label: A }}\n"
    );
    let ir = flowgraph_json(&raw);
    assert_eq!(ir["ir_version"], "flowgraph.v3");
    assert_eq!(ir["compile"]["compiler"], "zyalc");
    assert_eq!(ir["compile"]["timestamp_policy"], "omitted_for_determinism");
    for key in ["source_hash", "registry_hash", "params_hash"] {
        let h = ir["compile"][key].as_str().unwrap_or("");
        assert!(
            h.starts_with("sha256:") && h.len() == "sha256:".len() + 64,
            "bad {key}: {h}"
        );
    }
    // Changing the source changes only source_hash, never registry/params hash.
    let raw2 = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: b, type: agent, label: B }}\n"
    );
    let ir2 = flowgraph_json(&raw2);
    assert_ne!(ir["compile"]["source_hash"], ir2["compile"]["source_hash"]);
    assert_eq!(
        ir["compile"]["registry_hash"],
        ir2["compile"]["registry_hash"]
    );
    assert_eq!(ir["compile"]["params_hash"], ir2["compile"]["params_hash"]);
}

#[test]
fn flowgraph_flagship_matches_committed_golden() {
    // Regression snapshot: the flagship IR is blessed at `tests/golden/35.ir.json`.
    // When the IR shape intentionally changes, re-bless with:
    //   cargo run -p zyalc -- compile docs/ZYAL/examples/35-flowgraph-openqg-foundry.zyal \
    //     --out crates/zyalc/tests/golden/35.ir.json
    let src = format!(
        "{}/../../docs/ZYAL/examples/35-flowgraph-openqg-foundry.zyal",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden_path = format!("{}/tests/golden/35.ir.json", env!("CARGO_MANIFEST_DIR"));
    let raw = fs::read_to_string(&src).unwrap_or_else(|e| panic!("read {src}: {e}"));
    let fresh = emit_flowgraph(&raw).expect("emit flagship");
    let golden = fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("read golden {golden_path}: {e}"));
    assert_eq!(
        fresh, golden,
        "flagship IR drifted from the committed golden — re-bless if intentional"
    );
}

#[test]
fn node_types_registry_web_copy_is_byte_identical() {
    // The cockpit consumes a hand-maintained byte-identical copy of the registry.
    // Drift would desync render/validation. (Skips gracefully when jekko-web is
    // not checked out alongside, e.g. an isolated jekko clone.)
    let web = format!(
        "{}/../../../jekko-web/apps/web/src/lib/nodeTypes.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let Ok(web_bytes) = fs::read_to_string(&web) else {
        return;
    };
    assert_eq!(
        web_bytes,
        super::registry::REGISTRY_BYTES,
        "jekko-web nodeTypes.json drifted from contracts/node-types.json — re-copy it"
    );
}

#[test]
fn flowgraph_rejects_unbounded_dynamic_cardinality() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: pool, type: fanout, cardinality: {{ mode: discovered, identity: url }} }}\n"
    );
    let err = emit_flowgraph(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("ZYAL_E_DYNAMIC_CARDINALITY_UNBOUNDED"),
        "got: {err}"
    );
}

#[test]
fn flowgraph_bounded_dynamic_cardinality_emits_pending_group() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: pool, type: fanout, cardinality: {{ mode: discovered, max: 16, identity: url }} }}\n"
    );
    let ir = flowgraph_json(&raw);
    // The unresolved fan-out compiles to a `group.dynamic` placeholder with the
    // formal cardinality + runtime_state: pending (resolved live by a patch).
    let placeholders = node_ids_of_type(&ir, "group.dynamic");
    assert_eq!(
        placeholders.len(),
        1,
        "expected one group.dynamic placeholder"
    );
    let node = ir["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_type"] == "group.dynamic")
        .unwrap();
    assert_eq!(node["cardinality"]["mode"], "discovered");
    assert_eq!(node["cardinality"]["max"], 16);
    assert_eq!(node["cardinality"]["runtime_state"], "pending");
}

#[test]
fn flowgraph_mode_key_cardinality_matches_legacy_kind() {
    // The new `mode` key must expand identically to the legacy `kind` key:
    // `mode: config, value: 4` resolves to 4 concrete children (not a placeholder).
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: pool, type: spawn, spawn: {{ cardinality: {{ mode: config, value: 4 }} }} }}\n"
    );
    let ir = flowgraph_json(&raw);
    let agents = node_ids_of_type(&ir, "agent");
    assert_eq!(
        agents
            .iter()
            .filter(|id| id.starts_with("pool/tab-"))
            .count(),
        4,
        "mode:config must expand to concrete children like kind:config"
    );
    assert!(
        node_ids_of_type(&ir, "group.dynamic").is_empty(),
        "resolved cardinality must not emit a placeholder"
    );
}

#[test]
fn flowgraph_lowers_scatter_gather_and_loop_until_dry_patterns() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         patterns:\n\
         \x20 sweep:\n    kind: scatter_gather\n    split: {{ count: 3 }}\n\
         \x20   branch: {{ agent: researcher }}\n    gather: {{ agent: synth, reduce: merge }}\n\
         \x20 refine:\n    kind: loop_until_dry\n    body: {{ agent: refiner }}\n\
         \x20   stop: {{ marginal_utility_below: 0.05, max_rounds: 8 }}\n"
    );
    let ir = flowgraph_json(&raw);
    // scatter_gather → fanout split + 3 agent branches + fanin gather
    assert!(node_ids_of_type(&ir, "fanout")
        .iter()
        .all(|id| *id == "pattern/sweep/split"));
    let agents = node_ids_of_type(&ir, "agent");
    assert_eq!(
        agents
            .iter()
            .filter(|id| id.starts_with("pattern/sweep/branch-"))
            .count(),
        3
    );
    assert!(ir["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == "pattern/sweep/split" && n["node_type"] == "fanout"));
    assert!(ir["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == "pattern/sweep/gather" && n["node_type"] == "fanin"));
    // provenance is stamped
    let branch = ir["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "pattern/sweep/branch-0")
        .unwrap();
    assert_eq!(branch["pattern"]["id"], "sweep");
    assert_eq!(branch["pattern"]["template_node"], "branch");
    assert_eq!(branch["parent"], "pattern/sweep/split");
    // loop_until_dry → body + dry_certificate + a single loop_back edge
    assert_eq!(node_ids_of_type(&ir, "dry_certificate").len(), 1);
    let back = ir["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["loop_back"] == true
                && e["src"] == "pattern/refine/dry_certificate"
                && e["dst"] == "pattern/refine/body"
        })
        .count();
    assert_eq!(
        back, 1,
        "expected one loop_back from dry_certificate to body"
    );
}

#[test]
fn flowgraph_rejects_unbounded_pattern() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         patterns:\n  sweep:\n    kind: scatter_gather\n    branch: {{ agent: w }}\n"
    );
    let err = emit_flowgraph(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("ZYAL_E_PATTERN_UNBOUNDED_SCATTER"),
        "got: {err}"
    );
}

#[test]
fn flowgraph_rewrites_winner_port_edge_to_the_route_winner_node() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: route, type: router, router: {{ drafts: 2, fusion: 1 }} }}\n\
         \x20   - {{ id: sink, type: agent }}\n\
         \x20 edges:\n\
         \x20   - {{ from: route, to: sink, from_port: winner }}\n"
    );
    let ir = flowgraph_json(&raw);
    // the authored winner-port edge now originates from the real route_winner node
    let edge = ir["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["dst"] == "sink")
        .unwrap();
    assert_eq!(edge["src"], "route/winner");
    assert!(
        edge.get("from_port").is_none(),
        "from_port should be cleared after rewrite"
    );
}

#[test]
fn flowgraph_router_with_zero_drafts_has_no_orphan_judge() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: route, type: router, router: {{ drafts: 0, fusion: 1 }} }}\n"
    );
    let ir = flowgraph_json(&raw);
    assert!(
        node_ids_of_type(&ir, "route_judge").is_empty(),
        "drafts:0 must not orphan a route_judge"
    );
}

#[test]
fn flowgraph_rejects_unknown_authored_node_kind() {
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         graph:\n  nodes:\n\
         \x20   - {{ id: x, type: not_a_registered_kind }}\n"
    );
    let err = emit_flowgraph(&raw).unwrap_err();
    assert!(
        format!("{err}").contains("ZYAL_E_UNKNOWN_NODE_KIND"),
        "got: {err}"
    );
}

fn paper_builder_global_fixture() -> String {
    "# zyal: declarative target=flowgraph schema=zyal/flowgraph@1\n\
     exports:\n\
     \x20 - ref: zyal://global/paper-builder@1\n\
     \x20   interface:\n\
     \x20     inputs: [goal, data_artifacts, evidence, authors, success_criteria]\n\
     \x20     outputs: [tex, bib, figures, pdf, arxiv_tar, receipts]\n\
     id: global-paper-builder\n\
     job:\n\
     \x20 name: paper builder\n\
     \x20 objective: build papers\n"
        .to_string()
}

#[test]
fn flowgraph_resolves_uses_and_lowers_paper_builder() {
    let dir = tempfile::tempdir().unwrap();
    let global_dir = dir.path().join("agent/zyal/global");
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join("paper-builder.zyal"),
        paper_builder_global_fixture(),
    )
    .unwrap();
    let source = dir.path().join("agent/zyal/paper-flow.zyal");
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         uses:\n\
         \x20 - ref: zyal://global/paper-builder@1\n\
         \x20   as: paper_builder\n\
         paper_builder:\n\
         \x20 id: final_paper\n\
         \x20 use: zyal://global/paper-builder@1\n\
         \x20 mode: heavy\n\
         \x20 journal_target: ieee\n\
         \x20 paper_goal: \"show deterministic nested ZYAL calls\"\n\
         \x20 data_artifacts: [\"target/results/*.csv\"]\n\
         \x20 success_criteria: [\"latex_clean\"]\n\
         \x20 authors:\n\
         \x20   - name: Ada Lovelace\n\
         \x20     affiliation: Analytical Engine Lab\n\
         \x20     email: ada@example.org\n\
         \x20 output_dir: \"target/zyal/papers/${{run_id}}\"\n"
    );
    let rendered = emit_flowgraph_with_source(&raw, Some(&source)).expect("emit paper builder");
    let ir: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    let uses = ir["compile"]["uses"].as_array().expect("resolved uses");
    assert_eq!(uses[0]["ref"], "zyal://global/paper-builder@1");
    assert!(uses[0]["source_hash"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(uses[0]["interface_hash"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));

    let nodes = ir["nodes"].as_array().unwrap();
    let paper = nodes.iter().find(|n| n["id"] == "final_paper").unwrap();
    assert_eq!(paper["node_type"], "paper_builder");
    assert_eq!(
        paper["workflow_call"]["ref"],
        "zyal://global/paper-builder@1"
    );
    assert_eq!(paper["review_topology"]["jailgun_epochs"], 5);
    assert_eq!(paper["capabilities"]["forbidden"][0], "network.fetch");
    assert!(nodes.iter().any(|n| n["id"] == "final_paper/workflow_gate"));
    assert!(nodes.iter().any(|n| n["id"] == "final_paper/out/pdf"));
    assert_eq!(
        nodes
            .iter()
            .filter(|n| n["id"].as_str().unwrap_or("").contains("/reviewer-"))
            .count(),
        15
    );
}

#[test]
fn flowgraph_missing_use_ref_has_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("agent/zyal/missing.zyal");
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         uses:\n\
         \x20 - ref: zyal://global/nope@1\n\
         \x20   as: missing\n"
    );
    let err = emit_flowgraph_with_source(&raw, Some(&source)).unwrap_err();
    assert!(
        format!("{err:#}").contains("ZYAL_E_MISSING_REF"),
        "got {err:#}"
    );
}

#[test]
fn flowgraph_use_cycle_has_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let zyal_dir = dir.path().join("agent/zyal");
    fs::create_dir_all(&zyal_dir).unwrap();
    fs::write(
        zyal_dir.join("a.zyal"),
        "exports:\n  - ref: zyal://local/a@1\n    interface: { inputs: [], outputs: [] }\nuses:\n  - ref: zyal://local/b@1\n    as: b\nid: a\njob: { name: a, objective: a }\n",
    )
    .unwrap();
    fs::write(
        zyal_dir.join("b.zyal"),
        "exports:\n  - ref: zyal://local/b@1\n    interface: { inputs: [], outputs: [] }\nuses:\n  - ref: zyal://local/a@1\n    as: a\nid: b\njob: { name: b, objective: b }\n",
    )
    .unwrap();
    let source = zyal_dir.join("main.zyal");
    let raw = format!(
        "{FLOWGRAPH_HEADER}\
         uses:\n\
         \x20 - ref: zyal://local/a@1\n\
         \x20   as: a\n"
    );
    let err = emit_flowgraph_with_source(&raw, Some(&source)).unwrap_err();
    assert!(
        format!("{err:#}").contains("ZYAL_E_REF_CYCLE"),
        "got {err:#}"
    );
}
