use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use sha2::{Digest, Sha256};

use super::rust_symbols::{add_import_edges, add_rust_symbol_edges};
use super::scan::{add_test_edges, discover_files, is_test_file};
use super::{GraphEdge, GraphNode, RepoGraph};

/// Build a first-pass repo graph: files, docs, tests, Rust modules, packages,
/// and imports.
pub fn build_repo_graph(repo_root: &Path) -> Result<RepoGraph> {
    let mut builder = GraphBuilder::default();
    let files = discover_files(repo_root)?;
    let package_id = if repo_root.join("Cargo.toml").exists() {
        Some(builder.node("package", "Cargo.toml", "Cargo package"))
    } else {
        None
    };
    for rel in &files {
        let key = rel.to_string_lossy().replace('\\', "/");
        let kind = if key.ends_with(".md") || key.starts_with("docs/") {
            "doc"
        } else if is_test_file(&key) {
            "test"
        } else {
            "file"
        };
        let file_id = builder.node(kind, &key, &key);
        if let Some(package_id) = &package_id {
            builder.edge(package_id, &file_id, "contains");
        }
        if key.ends_with(".rs") {
            let module_id = builder.node("module", &key, &key);
            builder.edge(&module_id, &file_id, "contains");
            add_import_edges(repo_root, rel, &mut builder, &module_id)?;
            add_rust_symbol_edges(repo_root, rel, &key, &mut builder, &module_id)?;
        }
    }
    add_test_edges(&files, &mut builder);
    Ok(builder.finish())
}

#[derive(Default)]
pub(super) struct GraphBuilder {
    nodes_by_key: BTreeMap<(String, String), String>,
    nodes: Vec<GraphNode>,
    edges: BTreeMap<(String, String, String), Option<serde_json::Value>>,
}

impl GraphBuilder {
    pub(super) fn node(&mut self, kind: &str, key: &str, label: &str) -> String {
        self.node_inner(kind, key, label, None)
    }

    pub(super) fn node_with_payload(
        &mut self,
        kind: &str,
        key: &str,
        label: &str,
        payload_json: serde_json::Value,
    ) -> String {
        self.node_inner(kind, key, label, Some(payload_json))
    }

    pub(super) fn edge(&mut self, from: &str, to: &str, kind: &str) {
        self.edges
            .entry((from.to_string(), to.to_string(), kind.to_string()))
            .or_insert(None);
    }

    pub(super) fn edge_with_payload(
        &mut self,
        from: &str,
        to: &str,
        kind: &str,
        payload_json: serde_json::Value,
    ) {
        self.edges.insert(
            (from.to_string(), to.to_string(), kind.to_string()),
            Some(payload_json),
        );
    }

    fn node_inner(
        &mut self,
        kind: &str,
        key: &str,
        label: &str,
        payload_json: Option<serde_json::Value>,
    ) -> String {
        let lookup = (kind.to_string(), key.to_string());
        if let Some(id) = self.nodes_by_key.get(&lookup) {
            return id.clone();
        }
        let id = node_id(kind, key);
        self.nodes_by_key.insert(lookup, id.clone());
        self.nodes.push(GraphNode {
            id: id.clone(),
            kind: kind.to_string(),
            key: key.to_string(),
            label: label.to_string(),
            payload_json,
        });
        id
    }

    fn finish(self) -> RepoGraph {
        RepoGraph {
            nodes: self.nodes,
            edges: self
                .edges
                .into_iter()
                .map(|((from, to, kind), payload_json)| GraphEdge {
                    from,
                    to,
                    kind,
                    payload_json,
                })
                .collect(),
        }
    }
}

fn node_id(kind: &str, key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b":");
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}
