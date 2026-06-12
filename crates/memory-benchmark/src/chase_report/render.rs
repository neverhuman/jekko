use super::snapshot::same_identity;
use super::snapshot::{json_number, json_object, json_string};
use super::*;
pub(super) fn promotion_reason(
    raw_top: &CandidateSnapshot,
    selected: &CandidateSnapshot,
    current_best: &CandidateSnapshot,
    promoted: bool,
    delta: f64,
) -> String {
    if promoted {
        if same_identity(raw_top, selected) {
            format!(
                "best clean eligible lane clears the 0.75 threshold by {:.3} points",
                delta
            )
        } else {
            format!(
                "raw top {} blocked by hard gates; best clean eligible lane clears the 0.75 threshold",
                raw_top.name
            )
        }
    } else if same_identity(selected, current_best) {
        "no clean eligible lane beat the current best by 0.75 points".to_string()
    } else if selected.dev_only {
        "selected lane is dev_only and cannot promote".to_string()
    } else if delta < 0.75 {
        format!("selected lane improves by only {:.3} points", delta)
    } else {
        "promotion blocked".to_string()
    }
}

pub(super) fn hard_gate_delta(selected: &GateVector, current: &GateVector) -> Json {
    json::obj(&[
        (
            "unsafe_tool_exec",
            Json::Int(selected.unsafe_tool_exec as i64 - current.unsafe_tool_exec as i64),
        ),
        (
            "privacy_leaks",
            Json::Int(selected.privacy_leaks as i64 - current.privacy_leaks as i64),
        ),
        (
            "citation_issue_count",
            Json::Int(selected.citation_issue_count as i64 - current.citation_issue_count as i64),
        ),
        (
            "citation_issues",
            Json::Int(selected.citation_issues as i64 - current.citation_issues as i64),
        ),
        (
            "anomaly_citations",
            Json::Int(selected.anomaly_citations as i64 - current.anomaly_citations as i64),
        ),
        (
            "future_leaks",
            Json::Int(selected.future_leaks as i64 - current.future_leaks as i64),
        ),
        (
            "nondeterminism",
            Json::Int(selected.nondeterminism as i64 - current.nondeterminism as i64),
        ),
        (
            "determinism_failures",
            Json::Int(selected.determinism_failures as i64 - current.determinism_failures as i64),
        ),
        (
            "total",
            Json::Int(selected.total() as i64 - current.total() as i64),
        ),
    ])
}

pub(super) fn render_scoreboard(
    candidates: &[CandidateSnapshot],
    current_best: &CandidateSnapshot,
    selected: &CandidateSnapshot,
    raw_top: &CandidateSnapshot,
) -> String {
    let mut rows = String::from(
        "rank\tname\tsource\tci95_low\ttotal\tstress_total\tgate_count\tcost_usd\tdelta\tstatus\n",
    );
    for (index, candidate) in candidates.iter().enumerate() {
        let delta = candidate.score_key() - current_best.score_key();
        let status = if same_identity(candidate, current_best) {
            "current_best"
        } else if same_identity(candidate, selected) {
            "selected"
        } else if index == 0 && !candidate.gates.is_clean() {
            "blocked_top"
        } else if same_identity(candidate, raw_top) {
            "raw_top"
        } else {
            "candidate"
        };
        rows.push_str(&format!(
            "{}\t{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{}\t{:.2}\t{:.3}\t{}\n",
            index + 1,
            candidate.name,
            candidate.source,
            candidate.ci95_low,
            candidate.total,
            candidate.stress_total,
            candidate.gate_count(),
            candidate.cost_usd,
            delta,
            status,
        ));
    }
    rows
}

pub(super) fn render_negative_memory(
    candidates: &[CandidateSnapshot],
    selected: &CandidateSnapshot,
    current_best: &CandidateSnapshot,
    read_errors: &[ReadError],
    delta: f64,
) -> String {
    let mut out = String::new();
    for candidate in candidates {
        if same_identity(candidate, selected) || same_identity(candidate, current_best) {
            continue;
        }
        let reason = if !candidate.gates.is_clean() {
            "hard_gate_failure"
        } else if delta < 0.75 {
            "insufficient_margin"
        } else {
            "lower_ranking"
        };
        let line = json::obj(&[
            ("kind", Json::Str("negative-memory".to_string())),
            ("lane", Json::Str(candidate.name.clone())),
            ("source", Json::Str(candidate.source.clone())),
            ("reason", Json::Str(reason.to_string())),
            ("score", Json::Float(candidate.score_key())),
            ("gate_count", Json::Int(candidate.gate_count() as i64)),
            (
                "observed_at_run",
                candidate
                    .observed_at_run
                    .as_ref()
                    .map(|value| Json::Str(value.clone()))
                    .unwrap_or(Json::Null),
            ),
        ])
        .to_string();
        out.push_str(&line);
        out.push('\n');
    }
    for error in read_errors {
        if error.kind != ReadScope::LaneReport {
            continue;
        }
        let line = json::obj(&[
            ("kind", Json::Str("negative-memory".to_string())),
            ("lane", Json::Str(error.lane.clone())),
            ("source", Json::Str(error.source.clone())),
            ("reason", Json::Str("invalid_lane_report".to_string())),
            ("score", Json::Float(0.0)),
            ("gate_count", Json::Int(0)),
            (
                "observed_at_run",
                error
                    .observed_at_run
                    .as_ref()
                    .map(|value| Json::Str(value.clone()))
                    .unwrap_or(Json::Null),
            ),
        ])
        .to_string();
        out.push_str(&line);
        out.push('\n');
    }
    out
}

pub(super) fn render_curriculum(
    candidates: &[CandidateSnapshot],
    selected: &CandidateSnapshot,
    promoted: bool,
    delta: f64,
) -> Json {
    let proposals: Vec<Json> = candidates
        .iter()
        .filter(|candidate| !same_identity(candidate, selected))
        .take(5)
        .map(|candidate| {
            let next_step = if !candidate.gates.is_clean() {
                "repair gate failures before rerunning"
            } else if promoted {
                "use as a backup hypothesis"
            } else if delta < 0.75 {
                "increase evidence depth to raise ci95_low"
            } else {
                "strengthen the lane before promotion"
            };
            json::obj(&[
                ("lane", Json::Str(candidate.name.clone())),
                ("source", Json::Str(candidate.source.clone())),
                (
                    "hypothesis",
                    Json::Str(candidate.hypothesis.clone().unwrap_or_default()),
                ),
                ("next_step", Json::Str(next_step.to_string())),
            ])
        })
        .collect();
    json::obj(&[
        ("kind", Json::Str("curriculum-proposals".to_string())),
        ("proposals", Json::Array(proposals)),
    ])
}

pub(super) fn write_default_artifacts(out: Option<&str>) -> Result<(), String> {
    let Some(out) = out else {
        return Ok(());
    };
    let Some(parent) = Path::new(out).parent() else {
        return Ok(());
    };
    let artifacts = [
        (
            "axis-breakdown.json",
            json::obj(&[("kind", Json::Str("axis-breakdown".to_string()))]),
        ),
        (
            "gate-findings.json",
            json::obj(&[("kind", Json::Str("gate-findings".to_string()))]),
        ),
        (
            "support-minimality.json",
            json::obj(&[("kind", Json::Str("support-minimality".to_string()))]),
        ),
        (
            "privacy-audit.json",
            json::obj(&[("kind", Json::Str("privacy-audit".to_string()))]),
        ),
        (
            "economics.json",
            json::obj(&[("kind", Json::Str("economics".to_string()))]),
        ),
        (
            "bootstrap-ci.json",
            json::obj(&[("kind", Json::Str("bootstrap-ci".to_string()))]),
        ),
    ];
    for (name, body) in artifacts {
        let artifact_path = parent.join(name);
        let artifact_path = artifact_path.to_string_lossy().into_owned();
        write_file(Some(artifact_path.as_str()), &body.to_string())?;
    }
    Ok(())
}

pub(super) fn score_row(source: &str, score: &Json) -> Option<Json> {
    let obj = json_object(score)?;
    let name = json_string(obj, "name")?;
    let total = json_number(obj, "total")?;
    Some(json::obj(&[
        ("name", Json::Str(name)),
        ("source", Json::Str(source.to_string())),
        ("total", Json::Float(total)),
    ]))
}

pub(super) fn write_file(path: Option<&str>, content: &str) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    fs::write(path, content).map_err(|e| format!("write {}: {}", path, e))
}

pub(super) fn append_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("append {}: {}", path.display(), e))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("append {}: {}", path.display(), e))
}
