use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::builder::GraphBuilder;

pub(super) fn discover_files(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(repo_root, repo_root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), ".git" | ".zyal" | ".jankurai" | "target") {
                continue;
            }
            walk(root, &path, out)?;
        } else if path.is_file() {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
    Ok(())
}

pub(super) fn is_test_file(key: &str) -> bool {
    key.starts_with("tests/") || key.ends_with("_test.rs") || key.ends_with("_tests.rs")
}

pub(super) fn add_test_edges(files: &[PathBuf], builder: &mut GraphBuilder) {
    let source_files: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|key| key.starts_with("src/") && key.ends_with(".rs"))
        .collect();
    for test in files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|key| is_test_file(key))
    {
        let stem = Path::new(&test)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        for source in &source_files {
            if source.contains(stem) || stem == "integration" {
                let test_id = builder.node("test", &test, &test);
                let source_id = builder.node("file", source, source);
                builder.edge(&test_id, &source_id, "tests");
            }
        }
    }
}
