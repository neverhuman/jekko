//! Execute parity case lists against one or two adapters.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::adapters::TargetAdapter;
use super::types::{ParityCase, ParityReport};

/// Load parity cases from a directory of `.json` or `.toml` case files.
pub fn load_cases_from_dir(dir: &Path, approved_only: bool) -> Result<Vec<ParityCase>> {
    let mut cases = Vec::new();
    if !dir.exists() {
        return Ok(cases);
    }
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("read parity case dir {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|ext| ext.to_str());
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let case = match ext {
            Some("json") => serde_json::from_str::<ParityCase>(&text)
                .with_context(|| format!("parse {}", path.display()))?,
            Some("toml") => toml::from_str::<ParityCase>(&text)
                .with_context(|| format!("parse {}", path.display()))?,
            _ => continue,
        };
        if !approved_only || case.is_required() {
            cases.push(case);
        }
    }
    Ok(cases)
}

/// Return approved/required cases.
pub fn approved_cases(cases: &[ParityCase]) -> Vec<ParityCase> {
    cases
        .iter()
        .filter(|case| case.is_required())
        .cloned()
        .collect()
}

/// Run a case list against an adapter and return a report.
pub fn run_cases<A: TargetAdapter>(
    adapter: &mut A,
    cases: &[ParityCase],
    reference: &str,
    candidate: &str,
) -> Result<ParityReport> {
    adapter.setup()?;
    let mut results = Vec::new();
    for case in cases {
        results.push(adapter.run_case(case)?);
    }
    Ok(ParityReport {
        schema_version: "zyal.parity.v1".to_string(),
        reference: reference.to_string(),
        candidate: candidate.to_string(),
        results,
    })
}

/// Run cases against reference and candidate adapters.
pub fn run_target_switched_cases<A: TargetAdapter, B: TargetAdapter>(
    reference_adapter: &mut A,
    candidate_adapter: &mut B,
    cases: &[ParityCase],
) -> Result<ParityReport> {
    reference_adapter.setup()?;
    candidate_adapter.setup()?;
    let mut results = Vec::new();
    for case in cases {
        results.push(reference_adapter.run_case(case)?);
        results.push(candidate_adapter.run_case(case)?);
    }
    Ok(ParityReport {
        schema_version: "zyal.parity.v1".to_string(),
        reference: reference_adapter.name().to_string(),
        candidate: candidate_adapter.name().to_string(),
        results,
    })
}
