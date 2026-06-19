//! Path-overlap DAG over classified findings. Two findings have an edge when
//! their declared paths share a canonical prefix; the scheduler emits Kahn
//! topo waves so each wave is a set of mutually-disjoint findings the worker
//! pool can run in parallel without lock contention.
//!
//! Caps are always promoted to wave 0 in their own batch (and trip the
//! incubator lane downstream); putting a cap in a multi-finding wave would
//! defeat the purpose of treating caps as high-stakes work.

use std::collections::{BTreeMap, BTreeSet};

use crate::classifier::Finding;

#[derive(Debug, Clone)]
pub struct Wave {
    pub batches: Vec<Batch>,
}

#[derive(Debug, Clone)]
pub struct Batch {
    pub findings: Vec<Finding>,
}

impl Batch {
    pub fn touched_paths(&self) -> Vec<String> {
        let mut paths: BTreeSet<String> = BTreeSet::new();
        for f in &self.findings {
            for p in &f.paths {
                paths.insert(p.clone());
            }
        }
        paths.into_iter().collect()
    }
}

/// Splits findings into a sequence of waves. Each wave's batches do not share
/// paths with each other; later waves may overlap with earlier ones but never
/// run until the earlier wave drains. Caps always land alone in wave 0.
pub fn schedule(findings: &[Finding]) -> Vec<Wave> {
    if findings.is_empty() {
        return Vec::new();
    }

    let mut waves: Vec<Wave> = Vec::new();

    // Wave 0: caps, each in their own batch.
    let caps: Vec<Finding> = findings.iter().filter(|f| f.is_cap()).cloned().collect();
    if !caps.is_empty() {
        waves.push(Wave {
            batches: caps
                .into_iter()
                .map(|f| Batch { findings: vec![f] })
                .collect(),
        });
    }

    // Remaining findings (non-cap) ordered by severity desc, then by rule id
    // for stable test output.
    let mut remaining: Vec<Finding> = findings.iter().filter(|f| !f.is_cap()).cloned().collect();
    remaining.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.rule_id.cmp(&b.rule_id)));

    while !remaining.is_empty() {
        let (wave, leftover) = pack_one_wave(remaining);
        if wave.batches.is_empty() {
            // Defensive: never an infinite loop. Promote leftovers to a final
            // wave of singletons.
            waves.push(Wave {
                batches: leftover
                    .into_iter()
                    .map(|f| Batch { findings: vec![f] })
                    .collect(),
            });
            break;
        }
        waves.push(wave);
        remaining = leftover;
    }

    waves
}

fn pack_one_wave(findings: Vec<Finding>) -> (Wave, Vec<Finding>) {
    let mut claimed_paths: BTreeMap<String, usize> = BTreeMap::new();
    let mut batches: Vec<Batch> = Vec::new();
    let mut leftover: Vec<Finding> = Vec::new();

    for finding in findings {
        let conflicting = finding.paths.iter().any(|p| {
            claimed_paths
                .keys()
                .any(|claimed| paths_overlap(p, claimed))
        });
        if conflicting {
            leftover.push(finding);
        } else {
            let batch_index = batches.len();
            for p in &finding.paths {
                claimed_paths.insert(p.clone(), batch_index);
            }
            batches.push(Batch {
                findings: vec![finding],
            });
        }
    }

    (Wave { batches }, leftover)
}

fn paths_overlap(a: &str, b: &str) -> bool {
    let a = path_components(a);
    let b = path_components(b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    is_prefix(&a, &b) || is_prefix(&b, &a)
}

fn path_components(path: &str) -> Vec<String> {
    std::path::Path::new(path)
        .components()
        .filter_map(|component| match component {
            std::path::Component::CurDir => None,
            std::path::Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            std::path::Component::RootDir => Some("/".to_string()),
            std::path::Component::Prefix(value) => {
                Some(value.as_os_str().to_string_lossy().to_string())
            }
            std::path::Component::ParentDir => Some("..".to_string()),
        })
        .collect()
}

fn is_prefix(prefix: &[String], path: &[String]) -> bool {
    prefix.len() <= path.len() && prefix.iter().zip(path.iter()).all(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::Severity;

    fn finding(rule: &str, sev: Severity, paths: &[&str]) -> Finding {
        Finding {
            rule_id: rule.to_string(),
            fingerprint: rule.to_string(),
            severity: sev,
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
            cap: None,
        }
    }

    fn cap(id: &str, paths: &[&str]) -> Finding {
        Finding {
            rule_id: format!("cap:{}", id),
            fingerprint: format!("cap:{}", id),
            severity: Severity::Critical,
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
            cap: Some(id.to_string()),
        }
    }

    #[test]
    fn disjoint_findings_form_a_single_wave() {
        let findings = vec![
            finding("A", Severity::Low, &["src/a.rs"]),
            finding("B", Severity::Low, &["src/b.rs"]),
            finding("C", Severity::Low, &["src/c.rs"]),
        ];
        let waves = schedule(&findings);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].batches.len(), 3);
    }

    #[test]
    fn overlapping_paths_split_across_waves() {
        let findings = vec![
            finding("A", Severity::Medium, &["src/shared.rs"]),
            finding("B", Severity::Medium, &["src/shared.rs"]),
            finding("C", Severity::Medium, &["src/other.rs"]),
        ];
        let waves = schedule(&findings);
        assert_eq!(waves.len(), 2);
        // First wave packs A + C (disjoint paths), pushes B forward.
        assert_eq!(waves[0].batches.len(), 2);
        assert_eq!(waves[1].batches.len(), 1);
        assert_eq!(waves[1].batches[0].findings[0].rule_id, "B");
    }

    #[test]
    fn ancestor_and_descendant_paths_split_across_waves() {
        let findings = vec![
            finding("A", Severity::Medium, &["src"]),
            finding("B", Severity::Medium, &["src/lib.rs"]),
            finding("C", Severity::Medium, &["src2/lib.rs"]),
        ];
        let waves = schedule(&findings);
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0].batches.len(), 2);
        assert_eq!(waves[1].batches[0].findings[0].rule_id, "B");
    }

    #[test]
    fn path_prefix_strings_without_component_boundary_do_not_conflict() {
        let findings = vec![
            finding("A", Severity::Medium, &["src/foo"]),
            finding("B", Severity::Medium, &["src/foobar"]),
        ];
        let waves = schedule(&findings);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].batches.len(), 2);
    }

    #[test]
    fn caps_go_in_wave_zero_each_alone() {
        let findings = vec![
            cap("c1", &["agent/proof-lanes.toml"]),
            cap("c2", &["agent/audit-policy.toml"]),
            finding("R", Severity::Low, &["src/x.rs"]),
        ];
        let waves = schedule(&findings);
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0].batches.len(), 2);
        for batch in &waves[0].batches {
            assert_eq!(batch.findings.len(), 1);
            assert!(batch.findings[0].is_cap());
        }
        assert_eq!(waves[1].batches[0].findings[0].rule_id, "R");
    }

    #[test]
    fn higher_severity_packs_first() {
        let findings = vec![
            finding("Low", Severity::Low, &["src/shared.rs"]),
            finding("High", Severity::High, &["src/shared.rs"]),
        ];
        let waves = schedule(&findings);
        assert_eq!(waves[0].batches[0].findings[0].rule_id, "High");
        assert_eq!(waves[1].batches[0].findings[0].rule_id, "Low");
    }

    #[test]
    fn empty_findings_returns_empty_waves() {
        let waves = schedule(&[]);
        assert!(waves.is_empty());
    }

    #[test]
    fn batch_touched_paths_dedupes_and_sorts() {
        let batch = Batch {
            findings: vec![
                finding("A", Severity::Low, &["src/b.rs", "src/a.rs"]),
                finding("B", Severity::Low, &["src/a.rs"]),
            ],
        };
        assert_eq!(batch.touched_paths(), vec!["src/a.rs", "src/b.rs"]);
    }
}
