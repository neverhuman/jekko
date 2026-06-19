use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Repository graph node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    /// Stable node id.
    pub id: String,
    /// Node kind.
    pub kind: String,
    /// Stable key.
    pub key: String,
    /// Human-readable label.
    pub label: String,
    /// Node payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<serde_json::Value>,
}

/// Repository graph edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node id.
    pub from: String,
    /// Destination node id.
    pub to: String,
    /// Edge kind.
    pub kind: String,
    /// Edge payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_json: Option<serde_json::Value>,
}

/// Built repository graph.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoGraph {
    /// Nodes.
    pub nodes: Vec<GraphNode>,
    /// Edges.
    pub edges: Vec<GraphEdge>,
}

impl RepoGraph {
    /// Return tests known to cover a file key.
    pub fn tests_covering(&self, file_key: &str) -> Vec<&GraphNode> {
        let file_ids: BTreeSet<&str> = self
            .nodes
            .iter()
            .filter(|node| node.kind == "file" && node.key == file_key)
            .map(|node| node.id.as_str())
            .collect();
        let test_ids: BTreeSet<&str> = self
            .edges
            .iter()
            .filter(|edge| edge.kind == "tests" && file_ids.contains(edge.to.as_str()))
            .map(|edge| edge.from.as_str())
            .collect();
        self.nodes
            .iter()
            .filter(|node| test_ids.contains(node.id.as_str()))
            .collect()
    }

    /// Return graph summary counts by node kind.
    pub fn summary(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for node in &self.nodes {
            *counts.entry(node.kind.clone()).or_insert(0) += 1;
        }
        counts
    }

    /// Render a compact, deterministic blast-radius slice: the most-connected
    /// nodes (degree = incident edges, a proxy for change-impact) and, for
    /// files, how many tests cover them. Fed into brainstorm lanes so workers
    /// reason about what their changes would ripple into and what is protected
    /// by tests. Empty when the graph has no nodes.
    pub fn blast_radius_slice(&self, max_items: usize) -> String {
        if self.nodes.is_empty() || max_items == 0 {
            return String::new();
        }
        let mut degree: BTreeMap<&str, usize> = BTreeMap::new();
        for edge in &self.edges {
            *degree.entry(edge.from.as_str()).or_insert(0) += 1;
            *degree.entry(edge.to.as_str()).or_insert(0) += 1;
        }
        let mut ranked: Vec<(&GraphNode, usize)> = self
            .nodes
            .iter()
            .map(|node| (node, degree.get(node.id.as_str()).copied().unwrap_or(0)))
            .collect();
        // Highest blast radius first; ties broken by key for determinism.
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.key.cmp(&b.0.key)));
        ranked
            .into_iter()
            .take(max_items)
            .map(|(node, deg)| {
                let tests = if node.kind == "file" {
                    let covering = self.tests_covering(&node.key).len();
                    if covering == 0 {
                        " [no covering tests]".to_string()
                    } else {
                        format!(" [{covering} covering test(s)]")
                    }
                } else {
                    String::new()
                };
                format!("- {} ({}, blast~{}){}", node.label, node.kind, deg, tests)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Export the graph as pretty JSON.
    pub fn export_json(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }
}
