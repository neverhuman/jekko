use std::fs;

use tempfile::tempdir;

use super::*;

#[test]
fn builds_file_test_and_import_edges() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='x'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/ping.rs"),
        "use std::fmt;\nmod codec;\npub struct Ping;\npub enum Reply { Pong }\npub fn ping() { helper(); }\nfn helper() {}\nimpl Ping { pub fn run(&self) { ping(); self.private(); } fn private(&self) {} }\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();
    fs::write(dir.path().join("tests/ping.rs"), "#[test]\nfn ping() {}\n").unwrap();
    fs::create_dir_all(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("docs/spec.md"), "spec").unwrap();

    let graph = build_repo_graph(dir.path()).unwrap();
    let summary = graph.summary();
    assert_eq!(summary.get("test").copied(), Some(1));
    assert!(summary.get("doc").copied().unwrap_or(0) >= 1);
    assert!(!graph.tests_covering("src/ping.rs").is_empty());
    assert!(graph.edges.iter().any(|edge| edge.kind == "imports"));
    assert!(graph.nodes.iter().any(|node| node.kind == "function"));
    assert!(graph.nodes.iter().any(|node| node.kind == "struct"));
    assert!(graph.nodes.iter().any(|node| node.kind == "enum"));
    assert!(graph.nodes.iter().any(|node| node.kind == "method"));
    assert!(graph.edges.iter().any(|edge| edge.kind == "calls"));
}

#[test]
fn blast_radius_slice_ranks_by_degree_and_is_deterministic() {
    let node = |id: &str, kind: &str, key: &str, label: &str| GraphNode {
        id: id.into(),
        kind: kind.into(),
        key: key.into(),
        label: label.into(),
        payload_json: None,
    };
    let edge = |from: &str, to: &str| GraphEdge {
        from: from.into(),
        to: to.into(),
        kind: "calls".into(),
        payload_json: None,
    };
    let graph = RepoGraph {
        nodes: vec![
            node("n1", "file", "src/a.rs", "src/a.rs"),
            node("n2", "function", "fn_b", "b()"),
            node("n3", "file", "src/c.rs", "src/c.rs"),
        ],
        // n2 is the most-connected node (degree 3).
        edges: vec![edge("n1", "n2"), edge("n3", "n2"), edge("n2", "n1")],
    };
    let slice = graph.blast_radius_slice(2);
    let lines: Vec<&str> = slice.lines().collect();
    assert_eq!(lines.len(), 2, "max_items bound respected: {slice}");
    assert!(
        lines[0].contains("b()"),
        "highest-blast-radius node ranks first: {slice}"
    );
    // Deterministic across calls.
    assert_eq!(graph.blast_radius_slice(2), slice);
    // Empty graph yields an empty slice.
    assert!(RepoGraph::default().blast_radius_slice(5).is_empty());
}
