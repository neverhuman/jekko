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
    pub anomaly_citations: u32,
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
            + self.anomaly_citations
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
            || self.anomaly_citations > current.anomaly_citations
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
                "anomaly_citations",
                Json::Int(self.anomaly_citations as i64),
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

mod policy;
mod render;
mod selection;
mod snapshot;

use self::render::{append_file, score_row, write_default_artifacts, write_file};

pub use selection::{build_chase_outputs, read_current_best, read_report_dir};
