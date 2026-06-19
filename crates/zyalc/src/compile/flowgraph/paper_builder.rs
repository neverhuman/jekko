use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

impl super::Builder {
    pub(super) fn paper_builder_block(
        &mut self,
        root: &Map<String, Value>,
        uses: &super::super::uses::ResolvedUses,
    ) -> Result<()> {
        let Some(block) = root.get("paper_builder").and_then(Value::as_object) else {
            return Ok(());
        };
        let id = block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("paper_builder");
        if self.ids.contains(id) {
            return Err(anyhow!("duplicate flowgraph graph.node id {id}"));
        }
        let use_ref = block
            .get("use")
            .and_then(Value::as_str)
            .unwrap_or(super::super::uses::PAPER_BUILDER_REF);
        let resolved = uses
            .for_ref(use_ref)
            .or_else(|| uses.for_alias("paper_builder"))
            .ok_or_else(|| {
                anyhow!("ZYAL_E_MISSING_REF: paper_builder use `{use_ref}` was not resolved")
            })?;
        let mode = block.get("mode").and_then(Value::as_str).unwrap_or("light");
        let (epochs, reviewers) = paper_review_topology(mode);
        let output_dir = block
            .get("output_dir")
            .and_then(Value::as_str)
            .unwrap_or("target/zyal/papers/${run_id}");

        let gate = format!("{id}/workflow_gate");
        self.node(
            &gate,
            "budget_gate",
            "workflow budget",
            super::obj(&[("scope", json!("paper_builder"))]),
        );
        if self.ids.contains("supervisor") {
            self.edge("supervisor", &gate, "dispatches", None, 1.0);
        }

        let mut request = block.clone();
        request
            .entry("journal_target")
            .or_insert_with(|| json!("ieee"));
        request.entry("mode").or_insert_with(|| json!("light"));
        if !request.contains_key("paper_goal") {
            if let Some(goal) = request.get("goal").cloned() {
                request.insert("paper_goal".into(), goal);
            }
        }
        request.insert("workflow_ref".into(), json!(use_ref));

        let mut extra = Map::new();
        extra.insert("paper_builder".into(), Value::Object(request));
        extra.insert(
            "workflow_call".into(),
            json!({
                "tool_id": "workflow.call",
                "ref": use_ref,
                "alias": resolved.alias,
                "source_hash": resolved.source_hash,
                "interface_hash": resolved.interface_hash,
                "budget_usd": paper_mode_budget(mode),
            }),
        );
        extra.insert(
            "outputs".into(),
            json!({
                "tex": format!("{output_dir}/main.tex"),
                "bib": format!("{output_dir}/references.bib"),
                "figures": format!("{output_dir}/figures"),
                "pdf": format!("{output_dir}/paper.pdf"),
                "arxiv_tar": format!("{output_dir}/arxiv.tar.gz"),
                "receipts": format!("{output_dir}/build_receipt.json"),
            }),
        );
        extra.insert(
            "review_topology".into(),
            json!({
                "jailgun_epochs": epochs,
                "reviewers_per_epoch": reviewers,
            }),
        );
        extra.insert(
            "capabilities".into(),
            json!({ "requires": ["workflow.call"], "forbidden": ["network.fetch"] }),
        );
        extra.insert(
            "effects".into(),
            json!({ "side_effecting": true, "default": ["artifact.write", "render", "workflow.call"] }),
        );
        extra.insert("idempotency".into(), json!("idempotent"));
        self.node(id, "paper_builder", id, extra);
        self.edge(&gate, id, "workflow_call", None, 1.0);

        for epoch in 0..epochs {
            let review_id = format!("{id}/review_epoch-{}", epoch + 1);
            let mut spec = Map::new();
            spec.insert("id".into(), json!(review_id));
            spec.insert("type".into(), json!("spawn"));
            spec.insert(
                "spawn".into(),
                json!({ "cardinality": { "mode": "config", "value": reviewers } }),
            );
            self.node(
                &review_id,
                "spawn",
                &format!("review epoch {}", epoch + 1),
                super::obj(&[("review_epoch", json!(epoch + 1))]),
            );
            self.edge(id, &review_id, "reviewed_by", None, 0.8);
            self.expand_cardinality(&review_id, &spec, "spawn", "reviewer", 1);
        }

        for (suffix, label) in [
            ("tex", "TeX"),
            ("bib", "BibTeX"),
            ("figures", "EPS figures"),
            ("pdf", "PDF"),
            ("arxiv_tar", "arXiv tarball"),
            ("receipts", "Receipts"),
        ] {
            let artifact_id = format!("{id}/out/{suffix}");
            self.node(
                &artifact_id,
                "artifact",
                label,
                super::obj(&[(
                    "artifact",
                    json!({ "kind": suffix, "output_dir": output_dir }),
                )]),
            );
            self.edge(id, &artifact_id, "derived_from", None, 1.0);
        }
        Ok(())
    }
}

fn paper_review_topology(mode: &str) -> (u64, u64) {
    match mode {
        "medium" => (1, 3),
        "heavy" => (5, 3),
        _ => (0, 0),
    }
}

fn paper_mode_budget(mode: &str) -> f64 {
    match mode {
        "medium" => 3.0,
        "heavy" => 9.0,
        _ => 1.0,
    }
}
