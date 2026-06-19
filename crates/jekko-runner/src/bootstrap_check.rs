//! Read-only "is this repo ready for the runner?" check. Mirror of the TS
//! `packages/jekko/src/cli/cmd/jankurai/detect.ts` from PR1: same canonical
//! file list, same audit-policy minimums. The runner refuses to start when
//! `is_ready` returns `ok=false` and emits a `bootstrap_required` event so the
//! TUI panel (PR5) can surface the missing files.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CanonicalFile {
    pub rel: &'static str,
    pub required: bool,
}

/// The canonical surface the runner expects. Stays in sync with the TS
/// `CANONICAL_FILES` constant from PR1.
pub const CANONICAL_FILES: &[CanonicalFile] = &[
    CanonicalFile {
        rel: "agent/JANKURAI_STANDARD.md",
        required: true,
    },
    CanonicalFile {
        rel: "agent/audit-policy.toml",
        required: true,
    },
    CanonicalFile {
        rel: "agent/owner-map.json",
        required: true,
    },
    CanonicalFile {
        rel: "agent/test-map.json",
        required: true,
    },
    CanonicalFile {
        rel: "agent/proof-lanes.toml",
        required: true,
    },
    CanonicalFile {
        rel: "agent/boundaries.toml",
        required: true,
    },
    CanonicalFile {
        rel: "agent/tool-adoption.toml",
        required: false,
    },
    CanonicalFile {
        rel: ".jekko/agent/generated-zones.toml",
        required: false,
    },
];

#[derive(Debug, Clone)]
pub struct Readiness {
    pub ok: bool,
    pub missing_required: Vec<String>,
    pub missing_optional: Vec<String>,
    pub present: Vec<String>,
    pub policy: PolicyAudit,
}

#[derive(Debug, Clone)]
pub struct PolicyAudit {
    pub exists: bool,
    pub has_min_score: bool,
    pub fail_on: Vec<String>,
    pub advisory_on: Vec<String>,
    pub missing_fail_on: Vec<String>,
    pub missing_advisory_on: Vec<String>,
    pub ok: bool,
}

pub fn is_ready(repo_root: &Path) -> Readiness {
    let mut missing_required: Vec<String> = Vec::new();
    let mut missing_optional: Vec<String> = Vec::new();
    let mut present: Vec<String> = Vec::new();
    for file in CANONICAL_FILES {
        let abs = repo_root.join(file.rel);
        if abs.exists() {
            present.push(file.rel.to_string());
        } else if file.required {
            missing_required.push(file.rel.to_string());
        } else {
            missing_optional.push(file.rel.to_string());
        }
    }
    let policy = audit_policy(&repo_root.join("agent/audit-policy.toml"));
    let ok = missing_required.is_empty() && policy.ok;
    Readiness {
        ok,
        missing_required,
        missing_optional,
        present,
        policy,
    }
}

const REQUIRED_FAIL_ON: &[&str] = &["critical", "high"];
const REQUIRED_ADVISORY_ON: &[&str] = &["medium", "low"];

pub fn audit_policy(path: &Path) -> PolicyAudit {
    if !path.exists() {
        return PolicyAudit {
            exists: false,
            has_min_score: false,
            fail_on: Vec::new(),
            advisory_on: Vec::new(),
            missing_fail_on: REQUIRED_FAIL_ON.iter().map(|s| s.to_string()).collect(),
            missing_advisory_on: REQUIRED_ADVISORY_ON.iter().map(|s| s.to_string()).collect(),
            ok: false,
        };
    }
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => {
            return PolicyAudit {
                exists: true,
                has_min_score: false,
                fail_on: Vec::new(),
                advisory_on: Vec::new(),
                missing_fail_on: REQUIRED_FAIL_ON.iter().map(|s| s.to_string()).collect(),
                missing_advisory_on: REQUIRED_ADVISORY_ON.iter().map(|s| s.to_string()).collect(),
                ok: false,
            };
        }
    };
    let has_min_score = text
        .lines()
        .any(|line| line.trim_start().starts_with("min_score"));
    let fail_on = extract_string_array(&text, "fail_on")
        .into_iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let advisory_on = extract_string_array(&text, "advisory_on")
        .into_iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let missing_fail_on: Vec<String> = REQUIRED_FAIL_ON
        .iter()
        .filter(|sev| !fail_on.iter().any(|f| f == *sev))
        .map(|s| s.to_string())
        .collect();
    let missing_advisory_on: Vec<String> = REQUIRED_ADVISORY_ON
        .iter()
        .filter(|sev| !advisory_on.iter().any(|f| f == *sev))
        .map(|s| s.to_string())
        .collect();
    let ok = has_min_score && missing_fail_on.is_empty() && missing_advisory_on.is_empty();
    PolicyAudit {
        exists: true,
        has_min_score,
        fail_on,
        advisory_on,
        missing_fail_on,
        missing_advisory_on,
        ok,
    }
}

fn extract_string_array(text: &str, key: &str) -> Vec<String> {
    // Match `<key> = ["a", "b"]` on a single line. Deliberately mirrors the
    // narrow TS parser in `validate-policy.ts` — full TOML round-trip is the
    // jankurai auditor's job.
    for line in text.lines() {
        let trimmed = line.trim_start();
        let rest = match trimmed.strip_prefix(key) {
            Some(r) => r,
            None => continue,
        };
        let rest = rest.trim_start();
        let rest = match rest.strip_prefix('=') {
            Some(r) => r.trim_start(),
            None => continue,
        };
        let rest = match rest.strip_prefix('[') {
            Some(r) => r,
            None => continue,
        };
        let end = match rest.find(']') {
            Some(i) => i,
            None => continue,
        };
        let inner = &rest[..end];
        return inner
            .split(',')
            .map(|p| {
                p.trim()
                    .trim_start_matches('"')
                    .trim_end_matches('"')
                    .trim_start_matches('\'')
                    .trim_end_matches('\'')
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn touch(repo: &Path, rel: &str) {
        let abs = repo.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(abs, "").unwrap();
    }

    #[test]
    fn empty_repo_is_not_ready() {
        let dir = tempdir().unwrap();
        let readiness = is_ready(dir.path());
        assert!(!readiness.ok);
        let required = CANONICAL_FILES.iter().filter(|f| f.required).count();
        assert_eq!(readiness.missing_required.len(), required);
    }

    #[test]
    fn fully_scaffolded_repo_with_valid_policy_is_ready() {
        let dir = tempdir().unwrap();
        for file in CANONICAL_FILES {
            touch(dir.path(), file.rel);
        }
        // Replace the empty audit-policy with valid content.
        fs::write(
            dir.path().join("agent/audit-policy.toml"),
            r#"[decision]
min_score = 85
fail_on = ["critical", "high"]
advisory_on = ["medium", "low"]
"#,
        )
        .unwrap();
        let readiness = is_ready(dir.path());
        assert!(readiness.ok, "{:?}", readiness);
        assert!(readiness.policy.ok);
    }

    #[test]
    fn missing_fail_on_severity_blocks_readiness() {
        let dir = tempdir().unwrap();
        for file in CANONICAL_FILES {
            touch(dir.path(), file.rel);
        }
        fs::write(
            dir.path().join("agent/audit-policy.toml"),
            r#"[decision]
min_score = 85
fail_on = ["critical"]
advisory_on = ["medium", "low"]
"#,
        )
        .unwrap();
        let readiness = is_ready(dir.path());
        assert!(!readiness.ok);
        assert_eq!(readiness.policy.missing_fail_on, vec!["high".to_string()]);
    }

    #[test]
    fn missing_optional_file_does_not_block_readiness() {
        let dir = tempdir().unwrap();
        for file in CANONICAL_FILES {
            if file.required {
                touch(dir.path(), file.rel);
            }
        }
        fs::write(
            dir.path().join("agent/audit-policy.toml"),
            r#"[decision]
min_score = 85
fail_on = ["critical", "high"]
advisory_on = ["medium", "low"]
"#,
        )
        .unwrap();
        let readiness = is_ready(dir.path());
        assert!(readiness.ok);
        assert!(!readiness.missing_optional.is_empty());
    }

    #[test]
    fn array_parser_is_case_insensitive_via_lowercase() {
        let policy = "[decision]\nmin_score = 90\nfail_on = [\"Critical\", \"HIGH\"]\nadvisory_on = [\"Medium\", \"low\"]\n";
        let dir = tempdir().unwrap();
        for file in CANONICAL_FILES {
            if file.required {
                touch(dir.path(), file.rel);
            }
        }
        fs::write(dir.path().join("agent/audit-policy.toml"), policy).unwrap();
        let readiness = is_ready(dir.path());
        assert!(readiness.ok);
    }
}
