use super::*;
pub(super) fn snapshot_from_state_wrapper(
    path: &Path,
    value: &Json,
) -> Result<CandidateSnapshot, String> {
    let obj = json_object(value).ok_or_else(|| "invalid_lane_report".to_string())?;
    if let Some(inner) = obj.get("winner") {
        let source = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("current-best")
            .to_string();
        return snapshot_from_candidate_like(path, source, inner);
    }
    if let Some(inner) = obj.get("selected") {
        let source = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("current-best")
            .to_string();
        return snapshot_from_candidate_like(path, source, inner);
    }
    if let Some(inner) = obj.get("current") {
        let source = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("current-best")
            .to_string();
        return snapshot_from_candidate_like(path, source, inner);
    }
    Err("invalid_lane_report".to_string())
}

pub(super) fn snapshot_from_score_json(
    source: &str,
    default_name: &str,
    value: Option<&Json>,
) -> Option<CandidateSnapshot> {
    let json = value?;
    let obj = json_object(json)?;
    let source_name = if let Some(value) = json_string(obj, "source") {
        value
    } else {
        source.to_string()
    };
    let name = if let Some(value) = first_json_string(obj, &["name", "lane", "id"]) {
        value
    } else {
        default_name.to_string()
    };
    let total = json_number(obj, "total").unwrap_or(0.0);
    let ci95_low = json_number_in(obj, &["bootstrap_ci", "ci95_low"]).unwrap_or(total);
    let ci95_high = json_number_in(obj, &["bootstrap_ci", "ci95_high"]).unwrap_or(total);
    let stress_total =
        if let Some(value) = first_json_number(obj, &["stress_total", "stress_score"]) {
            value
        } else if let Some(value) = json_number_in(obj, &["stress", "total"]) {
            value
        } else {
            total
        };
    let gates = gate_vector_from_value(json);
    let cost_usd = if let Some(value) = json_number(obj, "cost_usd") {
        value
    } else if let Some(value) = json_number_in(obj, &["observability", "cost", "budget"]) {
        value
    } else {
        0.0
    };
    let hypothesis = json_string(obj, "hypothesis");
    let observed_at_run = json_string(obj, "observed_at_run");
    let patch = first_json_string(obj, &["patch", "best_patch"]);
    let dev_only = json_bool(obj, "dev_only").unwrap_or(false);

    Some(CandidateSnapshot {
        name,
        source: source_name,
        total,
        ci95_low,
        ci95_high,
        stress_total,
        gates,
        cost_usd,
        hypothesis,
        patch,
        observed_at_run,
        dev_only,
    })
}

pub(super) fn snapshot_from_json(path: &Path, json: &Json) -> Result<CandidateSnapshot, String> {
    let source = path.to_string_lossy().to_string();
    snapshot_from_candidate_like(path, source, json)
}

pub(super) fn snapshot_from_candidate_like(
    report_path: &Path,
    source: impl Into<String>,
    json: &Json,
) -> Result<CandidateSnapshot, String> {
    let source = source.into();
    let obj = json_object(json).ok_or_else(|| "invalid_lane_report".to_string())?;
    let source_name = if let Some(value) = json_string(obj, "source") {
        value
    } else {
        source.clone()
    };
    let name = if let Some(value) = first_json_string(obj, &["name", "lane", "id"]) {
        value
    } else {
        source_name.clone()
    };
    let total = json_number(obj, "total").unwrap_or(0.0);
    let ci95_low_raw = json_number_in(obj, &["bootstrap_ci", "ci95_low"]);
    let ci95_low = if let Some(v) = ci95_low_raw {
        if v < total {
            total
        } else {
            v
        }
    } else {
        total
    };
    let ci95_high = json_number_in(obj, &["bootstrap_ci", "ci95_high"]).unwrap_or(total);
    let stress_total =
        if let Some(value) = first_json_number(obj, &["stress_total", "stress_score"]) {
            value
        } else if let Some(value) = json_number_in(obj, &["stress", "total"]) {
            value
        } else {
            total
        };
    let gates = gate_vector_from_value(json);
    let cost_usd = if let Some(value) = json_number(obj, "cost_usd") {
        value
    } else if let Some(value) = json_number_in(obj, &["observability", "cost", "budget"]) {
        value
    } else {
        0.0
    };
    let hypothesis = json_string(obj, "hypothesis");
    let observed_at_run = json_string(obj, "observed_at_run");
    let dev_only = json_bool(obj, "dev_only").unwrap_or(false);
    let patch = if let Some(value) = first_json_string(obj, &["patch", "best_patch"]) {
        Some(value)
    } else {
        json_string(obj, "patch_path")
            .and_then(|patch_path| read_patch_content(report_path, &patch_path).ok())
    };
    if patch.is_none() && json_string(obj, "patch_path").is_some() {
        return Err("invalid_lane_report".to_string());
    }

    Ok(CandidateSnapshot {
        name,
        source: source_name,
        total,
        ci95_low,
        ci95_high,
        stress_total,
        gates,
        cost_usd,
        hypothesis,
        patch,
        observed_at_run,
        dev_only,
    })
}

pub(super) fn read_patch_content(report_path: &Path, patch_path: &str) -> Result<String, String> {
    let patch_path = Path::new(patch_path);
    if patch_path.is_absolute() {
        return Err(format!(
            "absolute patch path rejected: {}",
            patch_path.display()
        ));
    }
    let report_parent = report_path
        .parent()
        .ok_or_else(|| format!("patch path without parent: {}", report_path.display()))?;
    let report_parent = fs::canonicalize(report_parent)
        .map_err(|e| format!("canonicalize {}: {}", report_parent.display(), e))?;
    let resolved = report_parent.join(patch_path);
    let resolved = fs::canonicalize(&resolved)
        .map_err(|e| format!("patch path {}: {}", resolved.display(), e))?;
    if !resolved.starts_with(&report_parent) {
        return Err(format!(
            "patch path escaped report directory: {}",
            resolved.display()
        ));
    }
    fs::read_to_string(&resolved).map_err(|e| format!("read patch {}: {}", resolved.display(), e))
}

pub(super) fn gate_vector(findings: &BTreeMap<String, Json>) -> GateVector {
    let unsafe_tool_exec = gate_metric(findings, &["unsafe_tool_exec"]);
    let privacy_leaks = gate_metric(findings, &["privacy_leaks"]);
    let citation_issue_count = gate_metric(findings, &["citation_issue_count"]);
    let citation_issues = gate_metric(findings, &["citation_issues"]);
    let anomaly_citations = gate_metric(findings, &["anomaly_citations"]);
    let future_leaks = gate_metric(findings, &["future_leaks"]);
    let nondeterminism = gate_metric(findings, &["nondeterminism"]);
    let determinism_failures = gate_metric(findings, &["determinism_failures"])
        .max(matches_bool_false(findings.get("deterministic")));

    GateVector {
        unsafe_tool_exec,
        privacy_leaks,
        citation_issue_count,
        citation_issues,
        anomaly_citations,
        future_leaks,
        nondeterminism,
        determinism_failures,
    }
}

pub(super) fn matches_bool_false(value: Option<&Json>) -> u32 {
    match value {
        Some(Json::Bool(false)) => 1,
        _ => 0,
    }
}

pub(super) fn gate_metric(findings: &BTreeMap<String, Json>, keys: &[&str]) -> u32 {
    keys.iter()
        .filter_map(|key| findings.get(*key))
        .map(gate_count_from_value)
        .sum()
}

pub(super) fn gate_count_from_value(value: &Json) -> u32 {
    match value {
        Json::Bool(true) => 1,
        Json::Bool(false) => 0,
        Json::Int(v) if *v > 0 => *v as u32,
        Json::Float(v) if *v > 0.0 => v.round().max(1.0) as u32,
        Json::Array(items) => items.len() as u32,
        Json::Object(map) => map.len() as u32,
        Json::Str(value) if !value.is_empty() => 1,
        _ => 0,
    }
}

pub(super) fn gate_vector_from_value(json: &Json) -> GateVector {
    let Some(findings) = json_object(json)
        .and_then(|obj| obj.get("gate_findings"))
        .and_then(json_object)
    else {
        return GateVector::default();
    };
    gate_vector(findings)
}

pub(super) fn json_object(value: &Json) -> Option<&BTreeMap<String, Json>> {
    match value {
        Json::Object(map) => Some(map),
        _ => None,
    }
}

pub(super) fn json_string(obj: &BTreeMap<String, Json>, key: &str) -> Option<String> {
    match obj.get(key) {
        Some(Json::Str(value)) => Some(value.clone()),
        _ => None,
    }
}

pub(super) fn first_json_string(obj: &BTreeMap<String, Json>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = json_string(obj, key) {
            return Some(value);
        }
    }
    None
}

pub(super) fn json_number(obj: &BTreeMap<String, Json>, key: &str) -> Option<f64> {
    match obj.get(key) {
        Some(Json::Float(value)) => Some(*value),
        Some(Json::Int(value)) => Some(*value as f64),
        _ => None,
    }
}

pub(super) fn first_json_number(obj: &BTreeMap<String, Json>, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(value) = json_number(obj, key) {
            return Some(value);
        }
    }
    None
}

pub(super) fn json_bool(obj: &BTreeMap<String, Json>, key: &str) -> Option<bool> {
    match obj.get(key) {
        Some(Json::Bool(value)) => Some(*value),
        _ => None,
    }
}

pub(super) fn json_number_in(obj: &BTreeMap<String, Json>, path: &[&str]) -> Option<f64> {
    let mut current = obj;
    for key in &path[..path.len().saturating_sub(1)] {
        current = json_object(current.get(*key)?)?;
    }
    json_number(current, path.last()?)
}

pub(super) fn extract_observed_at_run(json: &Json) -> Option<String> {
    json_object(json).and_then(|obj| json_string(obj, "observed_at_run"))
}

pub(super) fn same_identity(left: &CandidateSnapshot, right: &CandidateSnapshot) -> bool {
    left.name == right.name && left.source == right.source
}

pub(super) fn is_eligible(candidate: &CandidateSnapshot, current_best: &CandidateSnapshot) -> bool {
    !candidate.dev_only
        && candidate.gates.is_clean()
        && candidate
            .patch
            .as_ref()
            .is_some_and(|patch| !patch.trim().is_empty())
        && !candidate
            .gates
            .has_new_failures_against(&current_best.gates)
        && candidate.score_key().is_finite()
}

pub(super) fn compare_candidates(left: &CandidateSnapshot, right: &CandidateSnapshot) -> Ordering {
    right
        .score_key()
        .partial_cmp(&left.score_key())
        .unwrap_or(Ordering::Equal)
        .then_with(|| {
            right
                .total
                .partial_cmp(&left.total)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| {
            right
                .stress_total
                .partial_cmp(&left.stress_total)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| left.gate_count().cmp(&right.gate_count()))
        .then_with(|| {
            left.cost_usd
                .partial_cmp(&right.cost_usd)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| left.source.cmp(&right.source))
        .then_with(|| left.name.cmp(&right.name))
}
