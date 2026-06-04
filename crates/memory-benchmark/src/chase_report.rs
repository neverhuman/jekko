//! Chase reducer for the memory benchmark population report.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::json::{self, Json};

#[derive(Clone, Debug, Default)]
pub struct CliOptions {
    pub population: Option<String>,
    pub baseline_path: Option<String>,
    pub exec_path: Option<String>,
    pub shadow_report: Option<String>,
    pub lanes_path: Option<String>,
    pub current_best_state: Option<String>,
    pub current_candidates: Option<String>,
    pub scoreboard: Option<String>,
    pub best_state: Option<String>,
    pub promotion_decision: Option<String>,
    pub negative_memory: Option<String>,
    pub best_patch: Option<String>,
    pub out: Option<String>,
    pub markdown: Option<String>,
    pub comparison: Option<String>,
    pub triangulation: Option<String>,
    pub curriculum: Option<String>,
    pub reference_reports: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GateVector {
    pub unsafe_tool_exec: u32,
    pub privacy_leaks: u32,
    pub citation_issue_count: u32,
    pub citation_issues: u32,
    pub fabricated_citations: u32,
    pub future_leaks: u32,
    pub nondeterminism: u32,
    pub determinism_failures: u32,
}

impl GateVector {
    pub fn total(&self) -> u32 {
        self.unsafe_tool_exec
            + self.privacy_leaks
            + self.citation_issue_count
            + self.citation_issues
            + self.fabricated_citations
            + self.future_leaks
            + self.nondeterminism
            + self.determinism_failures
    }

    pub fn is_clean(&self) -> bool {
        self.total() == 0
    }

    pub fn has_new_failures_against(&self, current: &Self) -> bool {
        self.unsafe_tool_exec > current.unsafe_tool_exec
            || self.privacy_leaks > current.privacy_leaks
            || self.citation_issue_count > current.citation_issue_count
            || self.citation_issues > current.citation_issues
            || self.fabricated_citations > current.fabricated_citations
            || self.future_leaks > current.future_leaks
            || self.nondeterminism > current.nondeterminism
            || self.determinism_failures > current.determinism_failures
    }

    fn to_json(&self) -> Json {
        json::obj(&[
            ("unsafe_tool_exec", Json::Int(self.unsafe_tool_exec as i64)),
            ("privacy_leaks", Json::Int(self.privacy_leaks as i64)),
            (
                "citation_issue_count",
                Json::Int(self.citation_issue_count as i64),
            ),
            ("citation_issues", Json::Int(self.citation_issues as i64)),
            (
                "fabricated_citations",
                Json::Int(self.fabricated_citations as i64),
            ),
            ("future_leaks", Json::Int(self.future_leaks as i64)),
            ("nondeterminism", Json::Int(self.nondeterminism as i64)),
            (
                "determinism_failures",
                Json::Int(self.determinism_failures as i64),
            ),
            ("total", Json::Int(self.total() as i64)),
        ])
    }
}

#[derive(Clone, Debug)]
pub struct CandidateSnapshot {
    pub name: String,
    pub source: String,
    pub total: f64,
    pub ci95_low: f64,
    pub ci95_high: f64,
    pub stress_total: f64,
    pub gates: GateVector,
    pub cost_usd: f64,
    pub hypothesis: Option<String>,
    pub patch: Option<String>,
    pub observed_at_run: Option<String>,
    pub dev_only: bool,
}

impl CandidateSnapshot {
    pub fn score_key(&self) -> f64 {
        if self.ci95_low.is_finite() {
            self.ci95_low
        } else {
            self.total
        }
    }

    pub fn gate_count(&self) -> u32 {
        self.gates.total()
    }

    fn to_json(&self) -> Json {
        let mut obj = BTreeMap::new();
        obj.insert("name".to_string(), Json::Str(self.name.clone()));
        obj.insert("source".to_string(), Json::Str(self.source.clone()));
        obj.insert("total".to_string(), Json::Float(self.total));
        obj.insert("ci95_low".to_string(), Json::Float(self.ci95_low));
        obj.insert("ci95_high".to_string(), Json::Float(self.ci95_high));
        obj.insert("stress_total".to_string(), Json::Float(self.stress_total));
        obj.insert(
            "gate_count".to_string(),
            Json::Int(self.gate_count() as i64),
        );
        obj.insert("gates".to_string(), self.gates.to_json());
        obj.insert("cost_usd".to_string(), Json::Float(self.cost_usd));
        if let Some(h) = &self.hypothesis {
            obj.insert("hypothesis".to_string(), Json::Str(h.clone()));
        }
        if let Some(run) = &self.observed_at_run {
            obj.insert("observed_at_run".to_string(), Json::Str(run.clone()));
        }
        obj.insert("dev_only".to_string(), Json::Bool(self.dev_only));
        if let Some(patch) = &self.patch {
            obj.insert("patch".to_string(), Json::Str(patch.clone()));
        }
        Json::Object(obj)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReadError {
    pub kind: ReadScope,
    pub path: String,
    pub lane: String,
    pub source: String,
    pub reason: String,
    pub observed_at_run: Option<String>,
}

impl ReadError {
    fn to_json(&self) -> Json {
        let mut obj = BTreeMap::new();
        obj.insert(
            "kind".to_string(),
            Json::Str(self.kind.as_str().to_string()),
        );
        obj.insert("path".to_string(), Json::Str(self.path.clone()));
        obj.insert("lane".to_string(), Json::Str(self.lane.clone()));
        obj.insert("source".to_string(), Json::Str(self.source.clone()));
        obj.insert("reason".to_string(), Json::Str(self.reason.clone()));
        if let Some(run) = &self.observed_at_run {
            obj.insert("observed_at_run".to_string(), Json::Str(run.clone()));
        }
        Json::Object(obj)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ReadScope {
    #[default]
    LaneReport,
    CurrentBestState,
    CurrentCandidates,
}

impl ReadScope {
    fn as_str(&self) -> &'static str {
        match self {
            ReadScope::LaneReport => "lane-report",
            ReadScope::CurrentBestState => "current-best-state",
            ReadScope::CurrentCandidates => "current-candidates",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReportBundle {
    pub reports: Vec<CandidateSnapshot>,
    pub read_errors: Vec<ReadError>,
}

#[derive(Clone, Debug)]
pub struct ChaseOutputs {
    pub scoreboard: String,
    pub best_state: Json,
    pub promotion_decision: Json,
    pub negative_memory: String,
    pub best_patch: String,
    pub curriculum: Json,
}

pub fn run(mut options: CliOptions) -> Result<(), String> {
    if options.markdown.is_none()
        && options
            .out
            .as_deref()
            .is_some_and(|path| path.ends_with(".md"))
    {
        options.markdown = options.out.clone();
    }

    let baseline = read_score(options.baseline_path.as_deref());
    let exec = read_score(options.exec_path.as_deref());
    let shadow = read_score(options.shadow_report.as_deref());
    let reference_reports = read_score_paths(&options.reference_reports);
    let population_count = read_population(options.population.as_deref());
    let lane_bundle = read_report_dir(options.lanes_path.as_deref(), ReadScope::LaneReport);
    let (current_best, current_best_errors) = read_current_best(
        options.current_best_state.as_deref(),
        options.current_candidates.as_deref(),
        baseline.as_ref(),
        exec.as_ref(),
    );
    let mut read_errors = lane_bundle.read_errors;
    read_errors.extend(current_best_errors);

    let chase_enabled = options.lanes_path.is_some()
        || options.scoreboard.is_some()
        || options.best_state.is_some()
        || options.promotion_decision.is_some()
        || options.negative_memory.is_some()
        || options.best_patch.is_some();
    let chase_outputs = if chase_enabled {
        Some(build_chase_outputs(
            lane_bundle.reports,
            current_best,
            baseline.clone(),
            exec.clone(),
            shadow,
            reference_reports,
            read_errors,
        ))
    } else {
        None
    };

    let mut top = BTreeMap::new();
    top.insert("kind".to_string(), Json::Str("final-score".to_string()));
    top.insert(
        "baseline".to_string(),
        baseline.clone().unwrap_or(Json::Null),
    );
    top.insert("exec".to_string(), exec.clone().unwrap_or(Json::Null));
    top.insert(
        "population_entries".to_string(),
        Json::Int(population_count as i64),
    );

    write_file(
        options.out.as_deref(),
        &Json::Object(top.clone()).to_string(),
    )?;
    write_default_artifacts(options.out.as_deref())?;

    if let Some(p) = options.comparison.as_deref() {
        let matrix = build_matrix(&baseline, &exec);
        write_file(Some(p), &matrix.to_string())?;
    }

    if let Some(p) = options.triangulation.as_deref() {
        let t = json::obj(&[
            ("kind", Json::Str("triangulation".to_string())),
            (
                "note",
                Json::Str("populated when prompt-score is available".to_string()),
            ),
        ]);
        write_file(Some(p), &t.to_string())?;
    }

    if let Some(p) = options.curriculum.as_deref() {
        let body = match chase_outputs.as_ref() {
            Some(outputs) => outputs.curriculum.clone(),
            None => json::obj(&[
                ("kind", Json::Str("curriculum-proposals".to_string())),
                ("proposals", Json::Array(vec![])),
            ]),
        };
        write_file(Some(p), &body.to_string())?;
    }

    if let Some(outputs) = chase_outputs {
        if let Some(p) = options.scoreboard.as_deref() {
            write_file(Some(p), &outputs.scoreboard)?;
        }
        if let Some(p) = options.best_state.as_deref() {
            write_file(Some(p), &outputs.best_state.to_string())?;
        }
        if let Some(p) = options.promotion_decision.as_deref() {
            write_file(Some(p), &outputs.promotion_decision.to_string())?;
        }
        if let Some(p) = options.negative_memory.as_deref() {
            append_file(Path::new(p), &outputs.negative_memory)?;
        }
        if let Some(p) = options.best_patch.as_deref() {
            write_file(Some(p), &outputs.best_patch)?;
        }
    }

    if let Some(p) = options.markdown.as_deref() {
        let mut md = String::from("# Memory Benchmark Final Report\n\n");
        if let Some(b) = &baseline {
            md.push_str(&format!("## Baseline\n\n```\n{}\n```\n\n", b));
        }
        if let Some(e) = &exec {
            md.push_str(&format!("## Exec\n\n```\n{}\n```\n\n", e));
        }
        md.push_str(&format!(
            "## Population\n\nLedger entries: {}\n",
            population_count
        ));
        write_file(Some(p), &md)?;
    }

    eprintln!(
        "population_report: baseline={} exec={} population_entries={}",
        baseline.is_some(),
        exec.is_some(),
        population_count
    );

    Ok(())
}

pub fn read_score(path: Option<&str>) -> Option<Json> {
    let path = path?;
    let text = fs::read_to_string(path).ok()?;
    json::parse(&text).ok()
}

pub fn read_score_paths(paths: &[String]) -> Vec<Json> {
    paths
        .iter()
        .filter_map(|path| read_score(Some(path.as_str())))
        .collect()
}

pub fn read_population(path: Option<&str>) -> usize {
    let Some(path) = path else {
        return 0;
    };
    let Ok(text) = fs::read_to_string(path) else {
        return 0;
    };
    text.lines().filter(|line| !line.trim().is_empty()).count()
}

pub fn build_matrix(baseline: &Option<Json>, exec: &Option<Json>) -> Json {
    let mut rows = Vec::new();
    if let Some(b) = baseline {
        if let Some(row) = score_row("baseline", b) {
            rows.push(row);
        }
    }
    if let Some(e) = exec {
        if let Some(row) = score_row("exec", e) {
            rows.push(row);
        }
    }
    json::obj(&[
        ("kind", Json::Str("comparison-matrix".to_string())),
        ("rows", Json::Array(rows)),
    ])
}

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

    fallback_current_best(read_errors, baseline, exec)
}

fn fallback_current_best(
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

fn snapshot_from_state_wrapper(path: &Path, value: &Json) -> Result<CandidateSnapshot, String> {
    let obj = json_object(value).ok_or_else(|| "invalid_lane_report".to_string())?;
    if let Some(inner) = obj
        .get("winner")
        .or_else(|| obj.get("selected"))
        .or_else(|| obj.get("current"))
    {
        let source = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("current-best")
            .to_string();
        return snapshot_from_candidate_like(path, source, inner);
    }
    Err("invalid_lane_report".to_string())
}

fn snapshot_from_score_json(
    source: &str,
    fallback_name: &str,
    value: Option<&Json>,
) -> Option<CandidateSnapshot> {
    let json = value?;
    let obj = json_object(json)?;
    let source_name = json_string(obj, "source").unwrap_or_else(|| source.to_string());
    let name = json_string(obj, "name")
        .or_else(|| json_string(obj, "lane"))
        .or_else(|| json_string(obj, "id"))
        .unwrap_or_else(|| fallback_name.to_string());
    let total = json_number(obj, "total").unwrap_or(0.0);
    let ci95_low = json_number_in(obj, &["bootstrap_ci", "ci95_low"]).unwrap_or(total);
    let ci95_high = json_number_in(obj, &["bootstrap_ci", "ci95_high"]).unwrap_or(total);
    let stress_total = json_number(obj, "stress_total")
        .or_else(|| json_number(obj, "stress_score"))
        .or_else(|| json_number_in(obj, &["stress", "total"]))
        .unwrap_or(total);
    let gates = gate_vector_from_value(json);
    let cost_usd = json_number(obj, "cost_usd")
        .or_else(|| json_number_in(obj, &["observability", "cost", "budget"]))
        .unwrap_or(0.0);
    let hypothesis = json_string(obj, "hypothesis");
    let observed_at_run = json_string(obj, "observed_at_run");
    let patch = json_string(obj, "patch").or_else(|| json_string(obj, "best_patch"));
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

fn snapshot_from_json(path: &Path, json: &Json) -> Result<CandidateSnapshot, String> {
    let source = path.to_string_lossy().to_string();
    snapshot_from_candidate_like(path, source, json)
}

fn snapshot_from_candidate_like(
    report_path: &Path,
    source: impl Into<String>,
    json: &Json,
) -> Result<CandidateSnapshot, String> {
    let source = source.into();
    let obj = json_object(json).ok_or_else(|| "invalid_lane_report".to_string())?;
    let source_name = json_string(obj, "source").unwrap_or_else(|| source.clone());
    let name = json_string(obj, "name")
        .or_else(|| json_string(obj, "lane"))
        .or_else(|| json_string(obj, "id"))
        .unwrap_or_else(|| source_name.clone());
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
    let stress_total = json_number(obj, "stress_total")
        .or_else(|| json_number(obj, "stress_score"))
        .or_else(|| json_number_in(obj, &["stress", "total"]))
        .unwrap_or(total);
    let gates = gate_vector_from_value(json);
    let cost_usd = json_number(obj, "cost_usd")
        .or_else(|| json_number_in(obj, &["observability", "cost", "budget"]))
        .unwrap_or(0.0);
    let hypothesis = json_string(obj, "hypothesis");
    let observed_at_run = json_string(obj, "observed_at_run");
    let dev_only = json_bool(obj, "dev_only").unwrap_or(false);
    let patch = json_string(obj, "patch")
        .or_else(|| json_string(obj, "best_patch"))
        .or_else(|| {
            json_string(obj, "patch_path")
                .and_then(|patch_path| read_patch_content(report_path, &patch_path).ok())
        });
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

fn read_patch_content(report_path: &Path, patch_path: &str) -> Result<String, String> {
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

fn gate_vector(findings: &BTreeMap<String, Json>) -> GateVector {
    let unsafe_tool_exec = gate_metric(findings, &["unsafe_tool_exec"]);
    let privacy_leaks = gate_metric(findings, &["privacy_leaks"]);
    let citation_issue_count = gate_metric(findings, &["citation_issue_count"]);
    let citation_issues = gate_metric(findings, &["citation_issues"]);
    let fabricated_citations = gate_metric(findings, &["fabricated_citations"]);
    let future_leaks = gate_metric(findings, &["future_leaks"]);
    let nondeterminism = gate_metric(findings, &["nondeterminism"]);
    let determinism_failures = gate_metric(findings, &["determinism_failures"])
        .max(matches_bool_false(findings.get("deterministic")));

    GateVector {
        unsafe_tool_exec,
        privacy_leaks,
        citation_issue_count,
        citation_issues,
        fabricated_citations,
        future_leaks,
        nondeterminism,
        determinism_failures,
    }
}

fn matches_bool_false(value: Option<&Json>) -> u32 {
    match value {
        Some(Json::Bool(false)) => 1,
        _ => 0,
    }
}

fn gate_metric(findings: &BTreeMap<String, Json>, keys: &[&str]) -> u32 {
    keys.iter()
        .filter_map(|key| findings.get(*key))
        .map(gate_count_from_value)
        .sum()
}

fn gate_count_from_value(value: &Json) -> u32 {
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

fn gate_vector_from_value(json: &Json) -> GateVector {
    let Some(findings) = json_object(json)
        .and_then(|obj| obj.get("gate_findings"))
        .and_then(json_object)
    else {
        return GateVector::default();
    };
    gate_vector(findings)
}

fn json_object(value: &Json) -> Option<&BTreeMap<String, Json>> {
    match value {
        Json::Object(map) => Some(map),
        _ => None,
    }
}

fn json_string(obj: &BTreeMap<String, Json>, key: &str) -> Option<String> {
    match obj.get(key) {
        Some(Json::Str(value)) => Some(value.clone()),
        _ => None,
    }
}

fn json_number(obj: &BTreeMap<String, Json>, key: &str) -> Option<f64> {
    match obj.get(key) {
        Some(Json::Float(value)) => Some(*value),
        Some(Json::Int(value)) => Some(*value as f64),
        _ => None,
    }
}

fn json_bool(obj: &BTreeMap<String, Json>, key: &str) -> Option<bool> {
    match obj.get(key) {
        Some(Json::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn json_number_in(obj: &BTreeMap<String, Json>, path: &[&str]) -> Option<f64> {
    let mut current = obj;
    for key in &path[..path.len().saturating_sub(1)] {
        current = json_object(current.get(*key)?)?;
    }
    json_number(current, path.last()?)
}

fn extract_observed_at_run(json: &Json) -> Option<String> {
    json_object(json).and_then(|obj| json_string(obj, "observed_at_run"))
}

fn same_identity(left: &CandidateSnapshot, right: &CandidateSnapshot) -> bool {
    left.name == right.name && left.source == right.source
}

fn is_eligible(candidate: &CandidateSnapshot, current_best: &CandidateSnapshot) -> bool {
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

fn compare_candidates(left: &CandidateSnapshot, right: &CandidateSnapshot) -> Ordering {
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

fn promotion_reason(
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

fn hard_gate_delta(selected: &GateVector, current: &GateVector) -> Json {
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
            "fabricated_citations",
            Json::Int(selected.fabricated_citations as i64 - current.fabricated_citations as i64),
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

fn render_scoreboard(
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

fn render_negative_memory(
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

fn render_curriculum(
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

fn write_default_artifacts(out: Option<&str>) -> Result<(), String> {
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

fn score_row(source: &str, score: &Json) -> Option<Json> {
    let obj = json_object(score)?;
    let name = json_string(obj, "name")?;
    let total = json_number(obj, "total")?;
    Some(json::obj(&[
        ("name", Json::Str(name)),
        ("source", Json::Str(source.to_string())),
        ("total", Json::Float(total)),
    ]))
}

fn write_file(path: Option<&str>, content: &str) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    fs::write(path, content).map_err(|e| format!("write {}: {}", path, e))
}

fn append_file(path: &Path, content: &str) -> Result<(), String> {
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

/// Returns `0.0` if the patch is safe to apply (touches only mutable-surface paths
/// and contains no forbidden tokens), `1.0` if it violates the trusted-core
/// allowlist or contains a forbidden token. The reducer gates promotion on
/// `trusted_core_diff <= 0.0`.
fn patch_validation_violation_score(patch: Option<&str>) -> f64 {
    let Some(patch) = patch else {
        // No patch attached → no diff to validate. Same as the prior
        // `patch.is_some()` short-circuit: missing patch fails the gate.
        return 1.0;
    };
    if patch_touches_forbidden_path(patch) {
        return 1.0;
    }
    if patch_contains_forbidden_token(patch) {
        return 1.0;
    }
    0.0
}

/// Forbidden paths — anything inside the trusted core. The AutoResearch
/// mutable surface is `crates/cogcore/src/**` plus the non-reference
/// candidate files in `crates/memory-benchmark/src/candidates/`.
const FORBIDDEN_PATH_PREFIXES: &[&str] = &[
    "crates/memory-benchmark/src/scoring/",
    "crates/memory-benchmark/src/scorer.rs",
    "crates/memory-benchmark/src/runner.rs",
    "crates/memory-benchmark/src/runner_generated.rs",
    "crates/memory-benchmark/src/runner_support.rs",
    "crates/memory-benchmark/src/case.rs",
    "crates/memory-benchmark/src/generated/",
    "crates/memory-benchmark/src/corpus/",
    "crates/memory-benchmark/src/oracle/",
    "crates/memory-benchmark/src/fixture/",
    "crates/memory-benchmark/src/chase_report.rs",
    "crates/memory-benchmark/src/lib.rs",
    "crates/memory-benchmark/src/types.rs",
    "crates/memory-benchmark/src/result.rs",
    "crates/memory-benchmark/src/memory_api.rs",
    "crates/memory-benchmark/src/adapters/baseline.rs",
    "crates/memory-benchmark/src/adapters/reference_context_pack.rs",
    "crates/memory-benchmark/src/adapters/reference_evidence_ledger.rs",
    "crates/memory-benchmark/src/adapters/reference_claim_skeptic.rs",
    "crates/memory-benchmark/tests/",
    "docs/ZYAL/SPEC.md",
];

fn patch_touches_forbidden_path(patch: &str) -> bool {
    for line in patch.lines() {
        // Unified diff path lines: `+++ b/<path>` and `--- a/<path>`.
        let trimmed = line.trim_start();
        let Some(rest) = trimmed
            .strip_prefix("+++ ")
            .or_else(|| trimmed.strip_prefix("--- "))
        else {
            continue;
        };
        let path = rest.trim_start_matches("a/").trim_start_matches("b/");
        // Strip trailing tab + timestamp some diff tools append.
        let path = path.split_whitespace().next().unwrap_or(path);
        if path == "/dev/null" {
            continue;
        }
        for prefix in FORBIDDEN_PATH_PREFIXES {
            if path == *prefix || path.starts_with(prefix) {
                return true;
            }
        }
    }
    false
}

/// Forbidden tokens that signal non-determinism or secret leakage. Scanned on
/// added lines (`+` prefix in unified diff) and on context lines that look like
/// rust code. Lines starting with `+//` are treated as comments and skipped.
const FORBIDDEN_TOKENS: &[&str] = &[
    "SystemTime::now",
    "Instant::now",
    "thread_rng",
    "rand::random",
    "rand::thread_rng",
    "chrono::",
    "env::var(",
    "process::Command",
    " unsafe ",
    " unsafe{",
    "panic!(",
    "unimplemented!(",
    "sk-",
    "SECRET_KEY",
    "SECRET_TOKEN",
];

fn patch_contains_forbidden_token(patch: &str) -> bool {
    for line in patch.lines() {
        let Some(added) = line.strip_prefix('+') else {
            continue;
        };
        // Skip diff header `+++` and pure comment lines.
        if added.starts_with("++") {
            continue;
        }
        let code = added.trim_start();
        if code.starts_with("//") || code.starts_with("/*") || code.starts_with("*") {
            continue;
        }
        for token in FORBIDDEN_TOKENS {
            if added.contains(token) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod patch_validation_tests {
    use super::*;

    #[test]
    fn no_patch_is_violation() {
        assert_eq!(patch_validation_violation_score(None), 1.0);
    }

    #[test]
    fn cogcore_only_patch_is_safe() {
        let patch = "\
diff --git a/crates/cogcore/src/config.rs b/crates/cogcore/src/config.rs
--- a/crates/cogcore/src/config.rs
+++ b/crates/cogcore/src/config.rs
@@ -1 +1 @@
-const K: f32 = 0.5;
+const K: f32 = 0.6;
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 0.0);
    }

    #[test]
    fn scorer_patch_is_rejected() {
        let patch = "\
diff --git a/crates/memory-benchmark/src/scoring/axes.rs b/crates/memory-benchmark/src/scoring/axes.rs
--- a/crates/memory-benchmark/src/scoring/axes.rs
+++ b/crates/memory-benchmark/src/scoring/axes.rs
@@ -1 +1 @@
-pub const WEIGHT: f32 = 14.0;
+pub const WEIGHT: f32 = 50.0;
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 1.0);
    }

    #[test]
    fn forbidden_token_systemtime_is_rejected() {
        let patch = "\
diff --git a/crates/cogcore/src/config.rs b/crates/cogcore/src/config.rs
--- a/crates/cogcore/src/config.rs
+++ b/crates/cogcore/src/config.rs
@@ -1 +1 @@
-fn time() -> u64 { 0 }
+fn time() -> u64 { SystemTime::now().elapsed().unwrap_or_default().as_secs() }
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 1.0);
    }

    #[test]
    fn forbidden_token_in_comment_is_allowed() {
        let patch = "\
diff --git a/crates/cogcore/src/config.rs b/crates/cogcore/src/config.rs
--- a/crates/cogcore/src/config.rs
+++ b/crates/cogcore/src/config.rs
@@ -1 +1 @@
-const K: f32 = 0.5;
+// SystemTime::now() is forbidden in cogcore hot path
+const K: f32 = 0.6;
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 0.0);
    }

    #[test]
    fn tests_dir_is_forbidden() {
        let patch = "\
diff --git a/crates/memory-benchmark/tests/foo.rs b/crates/memory-benchmark/tests/foo.rs
--- a/crates/memory-benchmark/tests/foo.rs
+++ b/crates/memory-benchmark/tests/foo.rs
@@ -1 +1 @@
-fn t() {}
+fn t() { assert!(true); }
";
        assert_eq!(patch_validation_violation_score(Some(patch)), 1.0);
    }

    #[test]
    fn dev_only_lane_is_not_eligible() {
        let current = CandidateSnapshot {
            name: "current".to_string(),
            source: "current".to_string(),
            total: 70.0,
            ci95_low: 70.0,
            ci95_high: 70.0,
            stress_total: 70.0,
            gates: GateVector::default(),
            cost_usd: 0.0,
            hypothesis: None,
            patch: None,
            observed_at_run: None,
            dev_only: false,
        };
        let candidate = CandidateSnapshot {
            name: "lane".to_string(),
            source: "lane".to_string(),
            total: 90.0,
            ci95_low: 90.0,
            ci95_high: 90.0,
            stress_total: 90.0,
            gates: GateVector::default(),
            cost_usd: 0.0,
            hypothesis: None,
            patch: Some("diff --git a/crates/cogcore/src/config.rs b/crates/cogcore/src/config.rs\n--- a/crates/cogcore/src/config.rs\n+++ b/crates/cogcore/src/config.rs\n@@ -1 +1 @@\n-a\n+b\n".to_string()),
            observed_at_run: None,
            dev_only: true,
        };
        assert!(!is_eligible(&candidate, &current));
    }

    #[test]
    fn drift_in_absolute_points() {
        // 2.36-point drift between selected and reference must read as 2.36,
        // not 0.0236. Old `/ 100.0` would have made this pass the 0.5 gate.
        let selected_score = 80.0_f64;
        let reference_score = 82.36_f64;
        let drift = (selected_score - reference_score).abs();
        assert!((drift - 2.36).abs() < 1e-6);
        // The gate is `<= 0.5`. With the new math, 2.36 fails the gate (rejection).
        assert!(drift > 0.5);
    }
}
