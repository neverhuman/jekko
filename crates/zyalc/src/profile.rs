//! Profile detection. `.zyal` files declare their profile via:
//! - Profile A — runbook: contains a `<<<ZYAL v1:` sentinel (no pragma).
//! - Profile B — declarative: first non-blank line is
//!   `# zyal: declarative target=toml schema=<name>@<ver>`.
//! - Profile C — workflow: first non-blank line is
//!   `# zyal: declarative target=github-workflow schema=actions/workflow@<ver>`.
//! - Profile D — SuperWorkflow: first non-blank line is
//!   `# zyal: declarative target=superworkflow schema=zyal/superworkflow@<ver>`.
//!
//! Anything else is rejected so silent format drift cannot happen.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Profile {
    /// Sentinel-wrapped runbook YAML; validated in place (the original
    /// `.zyal.yml` extension was retired in ZYAL contract 2.5.0).
    Runbook,
    /// Declarative → TOML.
    DeclarativeToml { schema: String },
    /// Declarative → GitHub Actions YAML.
    Workflow { schema: String },
    /// Declarative daemon runbook. Validated by `compile --all --check`, not emitted.
    Daemon { schema: String },
    /// Declarative SuperWorkflow manifest (9-12 macro phases). Emits canonical JSON.
    SuperWorkflow { schema: String },
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Profile::Runbook => write!(f, "runbook"),
            Profile::DeclarativeToml { .. } => write!(f, "declarative-toml"),
            Profile::Workflow { .. } => write!(f, "workflow"),
            Profile::Daemon { .. } => write!(f, "daemon"),
            Profile::SuperWorkflow { .. } => write!(f, "superworkflow"),
        }
    }
}

#[derive(Debug)]
pub struct ProfileInfo {
    pub profile: Profile,
    pub raw: String,
}

pub fn detect(path: &Path) -> Result<ProfileInfo> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let (profile, _consumed_pragma_bytes) = parse_header(&raw)?;
    Ok(ProfileInfo { profile, raw })
}

pub fn parse_header(raw: &str) -> Result<(Profile, usize)> {
    if let Some(pragma_line) = raw.lines().find(|l| {
        let trimmed = l.trim();
        !trimmed.is_empty() && trimmed.starts_with("# zyal:")
    }) {
        let attrs = parse_pragma(pragma_line)?;
        let target = match attrs.get("target") {
            Some(t) => t,
            None => return Err(anyhow!("pragma missing target= attribute: {pragma_line}")),
        };
        let schema = match attrs.get("schema") {
            Some(s) => s.clone(),
            None => "unspecified".into(),
        };
        let kind = attrs.get("").map(String::as_str).unwrap_or("declarative");
        if kind != "declarative" {
            return Err(anyhow!(
                "unsupported pragma kind '{kind}' in: {pragma_line}"
            ));
        }
        let profile = match target.as_str() {
            "toml" => Profile::DeclarativeToml { schema },
            "github-workflow" => Profile::Workflow { schema },
            "daemon" => Profile::Daemon { schema },
            "superworkflow" => Profile::SuperWorkflow { schema },
            other => return Err(anyhow!("unsupported target '{other}'")),
        };
        let pragma_offset = raw.find(pragma_line).unwrap_or(0) + pragma_line.len();
        return Ok((profile, pragma_offset));
    }
    if raw.contains("<<<ZYAL v1:") {
        return Ok((Profile::Runbook, 0));
    }
    Err(anyhow!(
        "unrecognised .zyal file: no pragma and no ZYAL sentinel"
    ))
}

fn parse_pragma(line: &str) -> Result<BTreeMap<String, String>> {
    // Format: `# zyal: <kind> key=value key=value ...`
    let body = match line
        .trim_start()
        .strip_prefix("#")
        .map(str::trim_start)
        .and_then(|s| s.strip_prefix("zyal:"))
        .map(str::trim)
    {
        Some(b) => b,
        None => return Err(anyhow!("malformed pragma: {line}")),
    };
    let mut parts = body.split_whitespace();
    let kind = match parts.next() {
        Some(k) => k,
        None => return Err(anyhow!("pragma missing kind: {line}")),
    };
    let mut map = BTreeMap::new();
    map.insert(String::new(), kind.to_string());
    for token in parts {
        let (k, v) = match token.split_once('=') {
            Some(pair) => pair,
            None => return Err(anyhow!("malformed pragma token '{token}' in: {line}")),
        };
        map.insert(k.to_string(), v.to_string());
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_declarative_toml() {
        let (p, _) = parse_header(
            "# zyal: declarative target=toml schema=jankurai/sandbox-lanes@1\nlanes:\n  - a",
        )
        .unwrap();
        assert!(matches!(p, Profile::DeclarativeToml { .. }));
    }

    #[test]
    fn detects_workflow() {
        let (p, _) = parse_header(
            "# zyal: declarative target=github-workflow schema=actions/workflow@1\non: push",
        )
        .unwrap();
        assert!(matches!(p, Profile::Workflow { .. }));
    }

    #[test]
    fn detects_runbook_sentinel() {
        let (p, _) = parse_header("<<<ZYAL v1:daemon id=test>>>\nversion: v1").unwrap();
        assert_eq!(p, Profile::Runbook);
    }

    #[test]
    fn rejects_unknown_target() {
        let err = parse_header("# zyal: declarative target=postgres schema=x@1").unwrap_err();
        assert!(format!("{err}").contains("unsupported target"));
    }

    #[test]
    fn detects_daemon_target() {
        let (p, _) = parse_header("# zyal: declarative target=daemon schema=runbook/smoke@1")
            .expect("daemon target");
        assert!(matches!(p, Profile::Daemon { .. }));
    }

    #[test]
    fn detects_superworkflow_target() {
        let (p, _) =
            parse_header("# zyal: declarative target=superworkflow schema=zyal/superworkflow@1")
                .expect("superworkflow target");
        assert!(matches!(p, Profile::SuperWorkflow { .. }));
    }

    #[test]
    fn rejects_no_pragma_no_sentinel() {
        let err = parse_header("just yaml\n  - 1").unwrap_err();
        assert!(format!("{err}").contains("unrecognised"));
    }
}
