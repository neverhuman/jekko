use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

use jekko_runner::hashing::sha256_hex;
use jekko_runner::hero_judge::HeroJudgeLaneMetric;

pub(super) fn file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

pub(super) fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).with_context(|| format!("decode {}", path.display()))
        })
        .collect()
}

pub(super) fn filter_series_lanes(
    metrics: &[HeroJudgeLaneMetric],
    role_group: &str,
) -> Vec<HeroJudgeLaneMetric> {
    metrics
        .iter()
        .filter(|metric| metric.role_group == role_group)
        .cloned()
        .collect()
}
