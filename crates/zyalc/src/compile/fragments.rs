//! Legacy-block → fragment provenance (M4-cont).
//!
//! ZYAL grew several convenience top-level blocks (`agents`, `fan_out`,
//! `dispatch`, `hero_judge`, the capability blocks, `patterns`) that the builder
//! lowers into concrete nodes. This records WHICH legacy blocks a document used
//! and the compiler pass that consumes each — an inspectable provenance trail
//! emitted as `fragments.blocks[]`.
//!
//! It is a TOPOLOGY no-op: nodes/edges are unchanged. The audit section is added
//! only when a legacy block is present, so a pure-authored graph (no legacy
//! blocks) keeps byte-identical IR. The list order is fixed, so the section is
//! deterministic. `node_prefix` is the canonical id prefix instances appear
//! under when the block lowers into topology.

use serde_json::{json, Map, Value};

struct LegacyBlock {
    key: &'static str,
    /// The compiler pass that consumes this block.
    lowering: &'static str,
    /// Canonical node-id prefix the block's instances appear under.
    node_prefix: &'static str,
}

const LEGACY_BLOCKS: &[LegacyBlock] = &[
    LegacyBlock {
        key: "agents",
        lowering: "synthesize",
        node_prefix: "worker-pool/",
    },
    LegacyBlock {
        key: "fan_out",
        lowering: "synthesize",
        node_prefix: "fan_out/",
    },
    LegacyBlock {
        key: "dispatch",
        lowering: "synthesize",
        node_prefix: "dispatch/",
    },
    LegacyBlock {
        key: "hero_judge",
        lowering: "tournament",
        node_prefix: "reasoning_lane/",
    },
    LegacyBlock {
        key: "watchers",
        lowering: "capability_block",
        node_prefix: "watcher/",
    },
    LegacyBlock {
        key: "kpi",
        lowering: "capability_block",
        node_prefix: "kpi/",
    },
    LegacyBlock {
        key: "data_feeds",
        lowering: "capability_block",
        node_prefix: "data_feed/",
    },
    LegacyBlock {
        key: "artifacts",
        lowering: "capability_block",
        node_prefix: "artifact/",
    },
    LegacyBlock {
        key: "patterns",
        lowering: "pattern_compiler",
        node_prefix: "pattern/",
    },
];

/// Build the `fragments` provenance section, or `None` when the document uses no
/// legacy block (so authored-only graphs keep byte-identical IR).
pub(super) fn normalize_legacy_blocks(root: &Map<String, Value>) -> Option<Value> {
    let blocks: Vec<Value> = LEGACY_BLOCKS
        .iter()
        .filter(|lb| root.contains_key(lb.key))
        .map(|lb| {
            json!({
                "name": lb.key,
                "lowering": lb.lowering,
                "node_prefix": lb.node_prefix,
            })
        })
        .collect();
    if blocks.is_empty() {
        return None;
    }
    Some(json!({ "blocks": blocks }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn authored_only_graph_has_no_fragments() {
        let r = root(json!({ "graph": { "nodes": [{ "id": "a", "type": "agent" }] } }));
        assert!(normalize_legacy_blocks(&r).is_none());
    }

    #[test]
    fn legacy_blocks_are_recorded_in_fixed_order() {
        let r = root(json!({ "hero_judge": { "generations": 3 }, "agents": {} }));
        let frags = normalize_legacy_blocks(&r).unwrap();
        let blocks = frags["blocks"].as_array().unwrap();
        let names: Vec<&str> = blocks.iter().map(|b| b["name"].as_str().unwrap()).collect();
        // declaration order in the source is irrelevant — fixed list order.
        assert_eq!(names, vec!["agents", "hero_judge"]);
        assert_eq!(blocks[1]["lowering"], "tournament");
    }

    #[test]
    fn is_deterministic() {
        let r = root(json!({ "patterns": {}, "watchers": [], "kpi": {} }));
        assert_eq!(normalize_legacy_blocks(&r), normalize_legacy_blocks(&r));
    }
}
