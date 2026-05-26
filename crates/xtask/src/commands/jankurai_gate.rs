use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::Value;

/// Path (relative to the repo root) where the gate reads its rule-level
/// overrides. Missing file means "no overrides" — gate behavior is
/// identical to the original code path.
pub const OVERRIDES_PATH: &str = "agent/jankurai-gate-overrides.toml";

pub fn run(score_path: &Path) -> Result<()> {
    let text =
        fs::read_to_string(score_path).with_context(|| format!("read {}", score_path.display()))?;
    let json: Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", score_path.display()))?;

    let score = json.get("score").and_then(Value::as_i64).unwrap_or(0);
    let minimum = json
        .get("minimum_score")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let reported = hard_findings(&json);

    // Walk the findings list and subtract entries that match an allow rule.
    let overrides_path = resolve_overrides_path(score_path);
    let overrides = load_overrides(&overrides_path)?;
    let waived = count_waived(&json, &overrides);
    let effective = (reported - waived).max(0);

    if waived > 0 {
        println!(
            "jankurai-gate: score={score} minimum={minimum} \
             hard_findings={reported} waived={waived} effective={effective} \
             (overrides: {})",
            overrides_path.display()
        );
    } else {
        println!("jankurai-gate: score={score} minimum={minimum} hard_findings={effective}");
    }

    if effective > 0 {
        bail!("jankurai gate failed: {effective} hard finding(s) (after {waived} waived)");
    }
    if minimum > 0 && score < minimum {
        bail!("jankurai gate failed: score {score} below minimum {minimum}");
    }
    Ok(())
}

fn hard_findings(json: &Value) -> i64 {
    if let Some(top) = json.get("hard_findings").and_then(Value::as_i64) {
        return top;
    }
    if let Some(nested) = json
        .get("decision")
        .and_then(|decision| decision.get("hard_findings"))
        .and_then(Value::as_i64)
    {
        return nested;
    }
    0
}

/// `agent/jankurai-gate-overrides.toml` relative to the repo root. We
/// derive the repo root from the score-path argument (typically
/// `.jankurai/repo-score.json` next to repo root), falling back to the
/// current working directory.
fn resolve_overrides_path(score_path: &Path) -> PathBuf {
    if let Some(parent) = score_path.parent().and_then(Path::parent) {
        return parent.join(OVERRIDES_PATH);
    }
    PathBuf::from(OVERRIDES_PATH)
}

#[derive(Debug, Default, Deserialize)]
struct Overrides {
    #[serde(default, rename = "allow")]
    allows: Vec<Allow>,
}

#[derive(Debug, Deserialize)]
struct Allow {
    rule: String,
    /// Optional substring filter on `finding.evidence[0]`.
    #[serde(default)]
    evidence: Option<String>,
    reason: String,
    /// `YYYY-MM-DD`. Gate refuses to apply expired overrides.
    expires: String,
}

fn load_overrides(path: &Path) -> Result<Overrides> {
    if !path.exists() {
        return Ok(Overrides::default());
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let parsed: Overrides =
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    let today = chrono::Utc::now().date_naive();
    for allow in &parsed.allows {
        if allow.reason.trim().is_empty() {
            bail!(
                "{}: allow entry for `{}` missing required `reason`",
                path.display(),
                allow.rule
            );
        }
        let expires =
            NaiveDate::parse_from_str(allow.expires.trim(), "%Y-%m-%d").with_context(|| {
                format!(
                    "{}: allow entry for `{}` has invalid `expires` (need YYYY-MM-DD)",
                    path.display(),
                    allow.rule
                )
            })?;
        if expires < today {
            bail!(
                "{}: allow entry for `{}` expired on {} — re-evaluate and bump or remove",
                path.display(),
                allow.rule,
                allow.expires
            );
        }
    }
    Ok(parsed)
}

fn count_waived(json: &Value, overrides: &Overrides) -> i64 {
    if overrides.allows.is_empty() {
        return 0;
    }
    let Some(findings) = json.get("findings").and_then(Value::as_array) else {
        return 0;
    };
    let mut count = 0i64;
    for finding in findings {
        let severity = finding
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("");
        if severity != "high" && severity != "critical" {
            continue;
        }
        let rule_id = finding.get("rule_id").and_then(Value::as_str).unwrap_or("");
        let first_evidence = finding
            .get("evidence")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(Value::as_str)
            .unwrap_or("");
        for allow in &overrides.allows {
            if allow.rule != rule_id {
                continue;
            }
            if let Some(needle) = &allow.evidence {
                if !first_evidence.contains(needle.as_str()) {
                    continue;
                }
            }
            count += 1;
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_score(json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f
    }

    #[test]
    fn no_overrides_file_means_zero_waived() {
        let score = write_score(
            r#"{"hard_findings":2,"findings":[
                {"severity":"high","rule_id":"HLT-001-DEAD-MARKER","evidence":["x.rs:1 unwrap_or_default"]},
                {"severity":"high","rule_id":"HLT-013-RENDERED-UX-GAP","evidence":["rendered UX QA lane missing"]}
            ]}"#,
        );
        let overrides = Overrides::default();
        let json: Value = serde_json::from_str(&fs::read_to_string(score.path()).unwrap()).unwrap();
        assert_eq!(count_waived(&json, &overrides), 0);
    }

    #[test]
    fn rule_only_allow_waives_all_matching_findings() {
        let json: Value = serde_json::from_str(
            r#"{"findings":[
                {"severity":"high","rule_id":"HLT-013-RENDERED-UX-GAP","evidence":["a"]},
                {"severity":"high","rule_id":"HLT-013-RENDERED-UX-GAP","evidence":["b"]},
                {"severity":"high","rule_id":"HLT-001-DEAD-MARKER","evidence":["c"]}
            ]}"#,
        )
        .unwrap();
        let overrides = Overrides {
            allows: vec![Allow {
                rule: "HLT-013-RENDERED-UX-GAP".into(),
                evidence: None,
                reason: "phantom flag".into(),
                expires: "2099-01-01".into(),
            }],
        };
        assert_eq!(count_waived(&json, &overrides), 2);
    }

    #[test]
    fn evidence_filter_narrows_match() {
        let json: Value = serde_json::from_str(
            r#"{"findings":[
                {"severity":"high","rule_id":"HLT-001-DEAD-MARKER","evidence":["x.rs:1 unwrap_or_default()"]},
                {"severity":"high","rule_id":"HLT-001-DEAD-MARKER","evidence":["y.rs:2 stub in product code"]}
            ]}"#,
        )
        .unwrap();
        let overrides = Overrides {
            allows: vec![Allow {
                rule: "HLT-001-DEAD-MARKER".into(),
                evidence: Some("unwrap_or_default".into()),
                reason: "rust idiom".into(),
                expires: "2099-01-01".into(),
            }],
        };
        // Only the first finding matches; the stub one is still counted as hard.
        assert_eq!(count_waived(&json, &overrides), 1);
    }

    #[test]
    fn non_high_severities_are_ignored() {
        let json: Value = serde_json::from_str(
            r#"{"findings":[
                {"severity":"medium","rule_id":"HLT-013-RENDERED-UX-GAP","evidence":[""]},
                {"severity":"low","rule_id":"HLT-013-RENDERED-UX-GAP","evidence":[""]}
            ]}"#,
        )
        .unwrap();
        let overrides = Overrides {
            allows: vec![Allow {
                rule: "HLT-013-RENDERED-UX-GAP".into(),
                evidence: None,
                reason: "x".into(),
                expires: "2099-01-01".into(),
            }],
        };
        assert_eq!(count_waived(&json, &overrides), 0);
    }
}
