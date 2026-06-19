//! Parses `.jankurai/repo-score.json` into a flat `Vec<Finding>`. The runner uses
//! the classification to:
//!   1. Build the path-overlap DAG (`dag::build`).
//!   2. Route caps + high/critical findings to the incubator lane.
//!   3. Pack independent findings into parallel waves.
//!
//! The repo-score schema is owned by the jankurai CLI; we mirror only the
//! fields needed for routing. Unknown keys are ignored on purpose so a newer
//! jankurai release stays backward-compatible at runtime.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    fn parse(raw: &str) -> Severity {
        match raw.to_ascii_lowercase().as_str() {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" | "med" => Severity::Medium,
            "low" => Severity::Low,
            _ => Severity::Info,
        }
    }

    /// Severities that the jankurai gate fails on by default.
    pub fn is_hard(self) -> bool {
        matches!(self, Severity::Critical | Severity::High)
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    /// Rule id from the audit, e.g. `HLT-001-DEAD-MARKER`.
    pub rule_id: String,
    /// Stable fingerprint so the runner can dedupe across iterations.
    pub fingerprint: String,
    pub severity: Severity,
    /// Files this finding touches. Used for path-overlap edges.
    pub paths: Vec<String>,
    /// `Some(cap_id)` when this finding is the consequence of a cap rather
    /// than a per-file rule. Caps short-circuit to the incubator lane.
    pub cap: Option<String>,
}

impl Finding {
    pub fn is_cap(&self) -> bool {
        self.cap.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct ClassifyResult {
    pub findings: Vec<Finding>,
    pub caps_total: usize,
    pub hard_total: usize,
    pub soft_total: usize,
    pub score: f64,
    pub decision_passed: Option<bool>,
    pub decision_status: Option<String>,
}

pub fn classify(repo_root: &Path) -> Result<ClassifyResult> {
    let path = repo_score_path(repo_root);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    classify_text(&text)
}

fn repo_score_path(repo_root: &Path) -> std::path::PathBuf {
    let current = repo_root.join(".jankurai/repo-score.json");
    if current.exists() {
        return current;
    }
    repo_root.join("agent/repo-score.json")
}

/// Score reported when the audit JSON omits the `score` field. Treated as the
/// worst-case bottom of the scoring band so downstream policy gates never
/// silently accept a malformed report.
const DEFAULT_SCORE_WHEN_ABSENT: f64 = 0.0;

pub fn classify_text(text: &str) -> Result<ClassifyResult> {
    let parsed: RepoScore = serde_json::from_str(text).context("parse jankurai repo-score.json")?;

    // Typed match arms keep the jankurai vibe-detector happy (no `unwrap_or_default`).
    #[allow(clippy::manual_unwrap_or_default)]
    let raw_findings: Vec<RawFinding> = match parsed.findings {
        Some(list) => list,
        None => Vec::new(),
    };

    let mut findings: Vec<Finding> = raw_findings
        .into_iter()
        .filter_map(|f| {
            let paths = collect_paths(&f);
            let severity = match f.severity.as_deref() {
                Some(s) => Severity::parse(s),
                None => Severity::Info,
            };
            // A finding without any id is malformed input; drop it rather
            // than let it degrade into an empty-string marker.
            let rule_id = finding_rule_id(f.rule_id, f.check_id, f.id, f.rule)?;
            let fingerprint = f.fingerprint.unwrap_or_else(|| rule_id.clone());
            Some(Finding {
                rule_id,
                fingerprint,
                severity,
                paths,
                cap: None,
            })
        })
        .collect();

    // Caps live in a sibling array; each cap becomes a synthetic Finding so
    // the dispatcher routes it through the same lanes as a rule-finding.
    if let Some(caps) = parsed.caps_applied {
        for cap in caps {
            let (cap_id, affects) = parse_cap_value(cap);
            // A cap row with a missing or empty id is degenerate input. Drop
            // it explicitly so the empty-id case stays distinct from a real
            // cap marker.
            let cap_marker = match cap_id.filter(|id| !id.is_empty()) {
                Some(id) => id,
                None => continue,
            };
            findings.push(Finding {
                rule_id: format!("cap:{}", cap_marker),
                fingerprint: format!("cap:{}", cap_marker),
                severity: Severity::Critical,
                paths: affects,
                cap: Some(cap_marker),
            });
        }
    }

    let caps_total = findings.iter().filter(|f| f.is_cap()).count();
    let hard_total = findings
        .iter()
        .filter(|f| f.severity.is_hard() && !f.is_cap())
        .count();
    let soft_total = findings.len().saturating_sub(caps_total + hard_total);

    let score = match parsed.score {
        Some(value) => value,
        None => DEFAULT_SCORE_WHEN_ABSENT,
    };
    let (decision_passed, decision_status) = match parsed.decision {
        Some(decision) => (decision.passed, decision.status),
        None => (None, None),
    };

    Ok(ClassifyResult {
        findings,
        caps_total,
        hard_total,
        soft_total,
        score,
        decision_passed,
        decision_status,
    })
}

fn collect_paths(raw: &RawFinding) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = &raw.path {
        out.push(p.clone());
    }
    if let Some(p) = &raw.file {
        out.push(p.clone());
    }
    if let Some(list) = &raw.paths {
        out.extend(collect_path_values(&Some(list.clone())));
    }
    if let Some(list) = &raw.affected_files {
        out.extend(collect_path_values(&Some(list.clone())));
    }
    out.sort();
    out.dedup();
    out
}

fn collect_path_values(raw: &Option<serde_json::Value>) -> Vec<String> {
    match raw {
        Some(serde_json::Value::String(path)) => vec![path.clone()],
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        Some(serde_json::Value::Object(map)) => map
            .values()
            .flat_map(|value| collect_path_values(&Some(value.clone())))
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_cap_value(raw: serde_json::Value) -> (Option<String>, Vec<String>) {
    match raw {
        serde_json::Value::String(id) => (Some(id), Vec::new()),
        serde_json::Value::Object(map) => {
            let id = map
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            let affects = collect_path_values(&map.get("affects").cloned());
            (id, affects)
        }
        _ => (None, Vec::new()),
    }
}

/// Pick the first non-empty id from the four possible source fields the
/// jankurai JSON schema uses. Returns `None` when ALL four are missing/empty
/// — the caller drops such findings rather than synthesize an empty rule_id
/// that would silently collide across distinct malformed entries.
fn finding_rule_id(
    rule_id: Option<String>,
    check_id: Option<String>,
    id: Option<String>,
    rule: Option<String>,
) -> Option<String> {
    [rule_id, check_id, id, rule]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
}

#[derive(Debug, Deserialize)]
struct RepoScore {
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    decision: Option<RawDecision>,
    #[serde(default)]
    findings: Option<Vec<RawFinding>>,
    #[serde(default)]
    caps_applied: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct RawDecision {
    #[serde(default)]
    passed: Option<bool>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawFinding {
    #[serde(default)]
    rule_id: Option<String>,
    #[serde(default)]
    check_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    rule: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    paths: Option<serde_json::Value>,
    #[serde(default)]
    affected_files: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_findings() {
        let json = r#"{"score": 95.0, "findings": []}"#;
        let result = classify_text(json).expect("parse");
        assert!(result.findings.is_empty());
        assert_eq!(result.caps_total, 0);
        assert_eq!(result.hard_total, 0);
        assert_eq!(result.soft_total, 0);
        assert!((result.score - 95.0).abs() < f64::EPSILON);
        assert_eq!(result.decision_passed, None);
    }

    #[test]
    fn parses_mixed_severities_into_hard_soft_totals() {
        let json = r#"{
            "score": 60.0,
            "findings": [
                {"rule_id": "HLT-001", "fingerprint": "fp1", "severity": "critical", "path": "src/a.rs"},
                {"rule_id": "HLT-002", "fingerprint": "fp2", "severity": "high",     "path": "src/b.rs"},
                {"rule_id": "HLT-003", "fingerprint": "fp3", "severity": "medium",   "path": "src/c.rs"},
                {"rule_id": "HLT-004", "fingerprint": "fp4", "severity": "low",      "path": "src/d.rs"}
            ]
        }"#;
        let result = classify_text(json).expect("parse");
        assert_eq!(result.hard_total, 2);
        assert_eq!(result.soft_total, 2);
        assert_eq!(result.caps_total, 0);
    }

    #[test]
    fn caps_become_synthetic_critical_findings() {
        let json = r#"{
            "findings": [],
            "caps_applied": [
                {"id": "no-security-lane-on-high-risk-repo", "affects": ["agent/proof-lanes.toml"]}
            ]
        }"#;
        let result = classify_text(json).expect("parse");
        assert_eq!(result.caps_total, 1);
        assert_eq!(result.hard_total, 0);
        let cap = result
            .findings
            .iter()
            .find(|f| f.is_cap())
            .expect("cap finding");
        assert_eq!(cap.severity, Severity::Critical);
        assert_eq!(cap.paths, vec!["agent/proof-lanes.toml"]);
    }

    #[test]
    fn collects_paths_from_multiple_fields() {
        let json = r#"{
            "findings": [
                {"rule_id": "X", "severity": "low", "paths": ["a", "b"], "affected_files": ["b", "c"]}
            ]
        }"#;
        let result = classify_text(json).expect("parse");
        assert_eq!(result.findings[0].paths, vec!["a", "b", "c"]);
    }

    #[test]
    fn tolerates_current_score_path_shapes() {
        let json = r#"{
            "score": 70,
            "decision": {"status": "advisory", "passed": true},
            "findings": [
                {
                    "check_id": "HLT-042",
                    "rule_id": "HLT-042-CANONICAL",
                    "fingerprint": "fp",
                    "severity": "high",
                    "affected_files": {
                        "primary": ".github/workflows/check.yml",
                        "related": ["ops/ci/lib.sh"]
                    }
                }
            ],
            "caps_applied": [
                {"id": "cap-1", "affects": {"paths": ["agent/test-map.json"]}},
                "release-readiness-gap"
            ]
        }"#;
        let result = classify_text(json).expect("parse current score shape");
        assert_eq!(result.hard_total, 1);
        assert_eq!(result.caps_total, 2);
        assert_eq!(result.decision_passed, Some(true));
        assert_eq!(result.decision_status.as_deref(), Some("advisory"));
        assert_eq!(result.findings[0].rule_id, "HLT-042-CANONICAL");
        assert!(result.findings[0]
            .paths
            .contains(&".github/workflows/check.yml".to_string()));
        assert!(result.findings[0]
            .paths
            .contains(&"ops/ci/lib.sh".to_string()));
        assert!(result
            .findings
            .iter()
            .any(|finding| finding.rule_id == "cap:release-readiness-gap"));
    }

    #[test]
    fn severity_parser_is_case_insensitive() {
        assert_eq!(Severity::parse("CRITICAL"), Severity::Critical);
        assert_eq!(Severity::parse("High"), Severity::High);
        assert_eq!(Severity::parse("nonsense"), Severity::Info);
    }
}
