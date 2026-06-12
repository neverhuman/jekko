use super::policy::patch_validation_violation_score;
use super::render::{
    hard_gate_delta, promotion_reason, render_curriculum, render_negative_memory, render_scoreboard,
};
use super::snapshot::{
    compare_candidates, extract_observed_at_run, is_eligible, json_object, same_identity,
    snapshot_from_json, snapshot_from_score_json, snapshot_from_state_wrapper,
};
use super::*;
pub fn read_report_dir(path: Option<&str>, kind: ReadScope) -> ReportBundle {
    let Some(path) = path else {
        return ReportBundle::default();
    };
    let root = Path::new(path);
    if !root.exists() {
        return ReportBundle::default();
    }

    let mut files = Vec::new();
    collect_json_files(root, &mut files);
    files.sort();

    let mut reports = Vec::new();
    let mut read_errors = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            read_errors.push(read_error(kind.clone(), &file, "invalid_lane_report", None));
            continue;
        };
        let Ok(json) = json::parse(&text) else {
            read_errors.push(read_error(kind.clone(), &file, "invalid_lane_report", None));
            continue;
        };
        match snapshot_from_json(&file, &json) {
            Ok(snapshot) => reports.push(snapshot),
            Err(_) => read_errors.push(read_error(
                kind.clone(),
                &file,
                "invalid_lane_report",
                extract_observed_at_run(&json),
            )),
        }
    }

    ReportBundle {
        reports,
        read_errors,
    }
}

pub fn read_current_best(
    path: Option<&str>,
    current_candidates: Option<&str>,
    baseline: Option<&Json>,
    exec: Option<&Json>,
) -> (CandidateSnapshot, Vec<ReadError>) {
    let mut read_errors = Vec::new();

    if let Some(path) = path {
        match fs::read_to_string(path) {
            Ok(text) => match json::parse(&text) {
                Ok(json) => {
                    let has_wrapper = json_object(&json).is_some_and(|obj| {
                        obj.contains_key("winner")
                            || obj.contains_key("selected")
                            || obj.contains_key("current")
                    });
                    if has_wrapper {
                        match snapshot_from_state_wrapper(Path::new(path), &json) {
                            Ok(snapshot) => return (snapshot, read_errors),
                            Err(_) => {
                                read_errors.push(read_error(
                                    ReadScope::CurrentBestState,
                                    Path::new(path),
                                    "invalid_current_best_state",
                                    extract_observed_at_run(&json),
                                ));
                            }
                        }
                    }
                    if let Ok(snapshot) = snapshot_from_json(Path::new(path), &json) {
                        return (snapshot, read_errors);
                    }
                    read_errors.push(read_error(
                        ReadScope::CurrentBestState,
                        Path::new(path),
                        "invalid_current_best_state",
                        extract_observed_at_run(&json),
                    ));
                }
                Err(_) => read_errors.push(read_error(
                    ReadScope::CurrentBestState,
                    Path::new(path),
                    "invalid_current_best_state",
                    None,
                )),
            },
            Err(_) => read_errors.push(read_error(
                ReadScope::CurrentBestState,
                Path::new(path),
                "invalid_current_best_state",
                None,
            )),
        }
    }

    if let Some(path) = current_candidates {
        let bundle = read_report_dir(Some(path), ReadScope::CurrentCandidates);
        let selected = select_best_candidate(bundle.reports.clone());
        read_errors.extend(bundle.read_errors);
        if let Some(snapshot) = selected {
            return (snapshot, read_errors);
        }
    }

    default_current_best(read_errors, baseline, exec)
}

fn default_current_best(
    read_errors: Vec<ReadError>,
    baseline: Option<&Json>,
    exec: Option<&Json>,
) -> (CandidateSnapshot, Vec<ReadError>) {
    if let Some(snapshot) = snapshot_from_score_json("baseline", "baseline", baseline) {
        return (snapshot, read_errors);
    }
    if let Some(snapshot) = snapshot_from_score_json("exec", "exec", exec) {
        return (snapshot, read_errors);
    }

    (
        CandidateSnapshot {
            name: "current-best".to_string(),
            source: "current-best".to_string(),
            total: 0.0,
            ci95_low: 0.0,
            ci95_high: 0.0,
            stress_total: 0.0,
            gates: GateVector::default(),
            cost_usd: 0.0,
            hypothesis: None,
            patch: None,
            observed_at_run: None,
            dev_only: false,
        },
        read_errors,
    )
}

pub fn build_chase_outputs(
    mut lane_reports: Vec<CandidateSnapshot>,
    current_best: CandidateSnapshot,
    baseline: Option<Json>,
    exec: Option<Json>,
    shadow: Option<Json>,
    reference_reports: Vec<Json>,
    read_errors: Vec<ReadError>,
) -> ChaseOutputs {
    let mut candidates = Vec::new();
    if lane_reports.is_empty() {
        if let Some(snapshot) = snapshot_from_score_json("baseline", "baseline", baseline.as_ref())
        {
            candidates.push(snapshot);
        }
        if let Some(snapshot) = snapshot_from_score_json("exec", "exec", exec.as_ref()) {
            candidates.push(snapshot);
        }
    } else {
        candidates.append(&mut lane_reports);
    }
    if !candidates
        .iter()
        .any(|candidate| same_identity(candidate, &current_best))
    {
        candidates.push(current_best.clone());
    }

    let mut ranked_all = candidates.clone();
    ranked_all.sort_by(compare_candidates);
    let raw_top = ranked_all
        .first()
        .cloned()
        .unwrap_or_else(|| current_best.clone());

    let mut eligible: Vec<CandidateSnapshot> = ranked_all
        .iter()
        .filter(|candidate| is_eligible(candidate, &current_best))
        .cloned()
        .collect();
    eligible.sort_by(compare_candidates);
    let selected = eligible
        .first()
        .cloned()
        .unwrap_or_else(|| current_best.clone());
    let shadow_snapshot = snapshot_from_score_json("shadow", "shadow", shadow.as_ref());
    let reference_snapshots: Vec<CandidateSnapshot> = reference_reports
        .iter()
        .filter_map(|report| snapshot_from_score_json("reference", "reference", Some(report)))
        .collect();

    let current_score = current_best.score_key();
    let selected_score = selected.score_key();
    let delta = selected_score - current_score;
    let shadow_delta = shadow_snapshot
        .as_ref()
        .map(|shadow| selected_score - shadow.score_key())
        .unwrap_or(0.0);
    let public_shadow_divergence = shadow_snapshot
        .as_ref()
        .map(|shadow| (selected_score - shadow.score_key()).abs())
        .unwrap_or(0.0);
    let reference_drift = reference_snapshots
        .iter()
        .map(|reference| (selected_score - reference.score_key()).abs())
        .fold(0.0, f64::max);
    let reference_mean = if reference_snapshots.is_empty() {
        0.0
    } else {
        reference_snapshots
            .iter()
            .map(|r| r.score_key())
            .sum::<f64>()
            / reference_snapshots.len() as f64
    };
    let trusted_core_diff = patch_validation_violation_score(selected.patch.as_deref());
    let promoted = !same_identity(&selected, &current_best)
        && selected.gates.is_clean()
        && !selected.gates.has_new_failures_against(&current_best.gates)
        && selected.patch.is_some()
        && delta >= 0.75;
    let promoted = promoted
        && shadow_delta >= 0.0
        && public_shadow_divergence <= 5.0
        && reference_drift <= 0.5
        && trusted_core_diff <= 0.0;
    let winner = if promoted {
        selected.clone()
    } else {
        current_best.clone()
    };

    let scoreboard = render_scoreboard(&ranked_all, &current_best, &selected, &raw_top);
    let read_errors_json = Json::Array(read_errors.iter().map(ReadError::to_json).collect());
    let promotion_reason = promotion_reason(&raw_top, &selected, &current_best, promoted, delta);
    let promotion_decision = json::obj(&[
        ("kind", Json::Str("promotion-decision".to_string())),
        (
            "decision",
            Json::Str(if promoted { "promote" } else { "reject" }.to_string()),
        ),
        ("reason", Json::Str(promotion_reason)),
        ("threshold", Json::Float(0.75)),
        ("raw_top", raw_top.to_json()),
        ("selected", selected.to_json()),
        ("current", current_best.to_json()),
        ("winner", winner.to_json()),
        ("score_delta", Json::Float(delta)),
        ("shadow_delta", Json::Float(shadow_delta)),
        (
            "public_shadow_divergence",
            Json::Float(public_shadow_divergence),
        ),
        ("reference_drift", Json::Float(reference_drift)),
        ("reference_mean", Json::Float(reference_mean)),
        ("trusted_core_diff", Json::Float(trusted_core_diff)),
        ("dev_only", Json::Bool(selected.dev_only)),
        ("eligible_lane_count", Json::Int(eligible.len() as i64)),
        (
            "blocked_lane_count",
            Json::Int(
                ranked_all
                    .iter()
                    .filter(|candidate| !is_eligible(candidate, &current_best))
                    .count() as i64,
            ),
        ),
        ("read_errors", read_errors_json.clone()),
        (
            "hard_gate_delta",
            hard_gate_delta(&selected.gates, &current_best.gates),
        ),
        ("current_score", Json::Float(current_score)),
        ("selected_score", Json::Float(selected_score)),
    ]);
    let best_state = json::obj(&[
        ("kind", Json::Str("best-state".to_string())),
        ("promoted", Json::Bool(promoted)),
        ("promotion_threshold", Json::Float(0.75)),
        (
            "ranking_rule",
            Json::Str("ci95_low, total, stress_total, gate_count, cost_usd".to_string()),
        ),
        ("raw_top", raw_top.to_json()),
        ("current", current_best.to_json()),
        ("selected", selected.to_json()),
        ("winner", winner.to_json()),
        ("score_delta", Json::Float(delta)),
        ("shadow_delta", Json::Float(shadow_delta)),
        (
            "public_shadow_divergence",
            Json::Float(public_shadow_divergence),
        ),
        ("reference_drift", Json::Float(reference_drift)),
        ("reference_mean", Json::Float(reference_mean)),
        ("trusted_core_diff", Json::Float(trusted_core_diff)),
        ("dev_only", Json::Bool(selected.dev_only)),
        ("eligible_lane_count", Json::Int(eligible.len() as i64)),
        (
            "blocked_lane_count",
            Json::Int(
                ranked_all
                    .iter()
                    .filter(|candidate| !is_eligible(candidate, &current_best))
                    .count() as i64,
            ),
        ),
        (
            "hard_gate_delta",
            hard_gate_delta(&selected.gates, &current_best.gates),
        ),
        ("read_errors", read_errors_json),
    ]);
    let negative_memory =
        render_negative_memory(&ranked_all, &selected, &current_best, &read_errors, delta);
    let best_patch = if promoted {
        selected.patch.clone().unwrap_or_default()
    } else {
        String::new()
    };
    let curriculum = render_curriculum(&ranked_all, &selected, promoted, delta);

    ChaseOutputs {
        scoreboard,
        best_state,
        promotion_decision,
        negative_memory,
        best_patch,
        curriculum,
    }
}

fn read_error(
    kind: ReadScope,
    path: &Path,
    reason: &str,
    observed_at_run: Option<String>,
) -> ReadError {
    let lane = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_string())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    ReadError {
        kind,
        path: path.to_string_lossy().to_string(),
        lane,
        source: path.to_string_lossy().to_string(),
        reason: reason.to_string(),
        observed_at_run,
    }
}

fn collect_json_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_json_files(&entry.path(), out);
    }
}

fn select_best_candidate(mut candidates: Vec<CandidateSnapshot>) -> Option<CandidateSnapshot> {
    candidates.sort_by(compare_candidates);
    candidates.into_iter().next()
}
