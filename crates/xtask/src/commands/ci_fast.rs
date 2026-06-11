//! `xtask ci-fast` — run fmt/clippy/test, the split-family registry check, and
//! all parity gates in sequence, fail fast.
//!
//! Mirrors the local-CI hook flow documented in `docs/ci-local.md`:
//!
//! 1. `cargo fmt --all -- --check`
//! 2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
//! 3. `xtask split-family-check`
//! 4. `cargo test --workspace --locked --no-fail-fast`
//! 5. `xtask db-migration-smoke`
//! 6. `xtask cli-help-parity --strict`
//! 7. `xtask tool-schema-parity --strict`
//! 8. `xtask session-fixture-parity --strict`
//! 9. `xtask openapi-check --strict`
//! 10. `xtask httpapi-parity --strict`
//! 11. `xtask baseline-diff --threshold 80`
//!
//! Each step prints its start/end timestamps and total duration. The
//! first non-zero exit short-circuits the rest of the pipeline.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chrono::Local;

/// One step in the pipeline.
struct Step {
    label: &'static str,
    program: &'static str,
    args: &'static [&'static str],
}

const STEPS: &[Step] = &[
    Step {
        label: "fmt",
        program: "cargo",
        args: &["fmt", "--all", "--", "--check"],
    },
    Step {
        label: "clippy",
        program: "cargo",
        args: &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
    },
    Step {
        label: "split-family-check",
        program: "cargo",
        args: &["run", "-p", "xtask", "--quiet", "--", "split-family-check"],
    },
    Step {
        label: "test",
        program: "cargo",
        args: &["test", "--workspace", "--locked", "--no-fail-fast"],
    },
    Step {
        label: "db-migration-smoke",
        program: "cargo",
        args: &["run", "-p", "xtask", "--quiet", "--", "db-migration-smoke"],
    },
    Step {
        label: "cli-help-parity",
        program: "cargo",
        args: &[
            "run",
            "-p",
            "xtask",
            "--quiet",
            "--",
            "cli-help-parity",
            "--strict",
        ],
    },
    Step {
        label: "tool-schema-parity",
        program: "cargo",
        args: &[
            "run",
            "-p",
            "xtask",
            "--quiet",
            "--",
            "tool-schema-parity",
            "--strict",
        ],
    },
    Step {
        label: "session-fixture-parity",
        program: "cargo",
        args: &[
            "run",
            "-p",
            "xtask",
            "--quiet",
            "--",
            "session-fixture-parity",
            "--strict",
        ],
    },
    Step {
        label: "openapi-check",
        program: "cargo",
        args: &[
            "run",
            "-p",
            "xtask",
            "--quiet",
            "--",
            "openapi-check",
            "--strict",
        ],
    },
    Step {
        label: "httpapi-parity",
        program: "cargo",
        args: &[
            "run",
            "-p",
            "xtask",
            "--quiet",
            "--",
            "httpapi-parity",
            "--strict",
        ],
    },
    Step {
        label: "baseline-diff-threshold-80",
        program: "cargo",
        args: &[
            "run",
            "-p",
            "xtask",
            "--quiet",
            "--",
            "baseline-diff",
            "--threshold",
            "80",
        ],
    },
];

/// Run the full pipeline. Returns `Err` (with the failing step's label)
/// on the first non-zero exit status.
pub fn run(repo_root: &Path) -> Result<()> {
    let overall_start = Instant::now();
    println!(
        "ci-fast: pipeline start at {}",
        Local::now().format("%Y-%m-%d %H:%M:%S")
    );

    for step in STEPS {
        let start = Instant::now();
        println!(
            "ci-fast: ▶ {} ({}) at {}",
            step.label,
            step.program,
            Local::now().format("%H:%M:%S")
        );

        let status = Command::new(step.program)
            .args(step.args)
            .current_dir(repo_root)
            .status()
            .with_context(|| format!("spawn `{} {}`", step.program, step.args.join(" ")))?;

        let elapsed = start.elapsed();
        println!(
            "ci-fast: {} {} in {}",
            if status.success() { "✓" } else { "✗" },
            step.label,
            format_duration(elapsed)
        );
        if !status.success() {
            let total = overall_start.elapsed();
            println!(
                "ci-fast: pipeline aborted after {} (failing step: {})",
                format_duration(total),
                step.label
            );
            bail!("ci-fast: step `{}` failed with exit {}", step.label, status);
        }
    }

    let total = overall_start.elapsed();
    println!("ci-fast: ✓ all steps passed in {}", format_duration(total));
    Ok(())
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let millis = d.subsec_millis();
    let minutes = secs / 60;
    let seconds = secs % 60;
    if minutes > 0 {
        format!("{minutes}m{seconds}.{millis:03}s")
    } else {
        format!("{seconds}.{millis:03}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_list_has_expected_labels() {
        let labels: Vec<&str> = STEPS.iter().map(|s| s.label).collect();
        assert_eq!(
            labels,
            vec![
                "fmt",
                "clippy",
                "split-family-check",
                "test",
                "db-migration-smoke",
                "cli-help-parity",
                "tool-schema-parity",
                "session-fixture-parity",
                "openapi-check",
                "httpapi-parity",
                "baseline-diff-threshold-80",
            ]
        );
    }

    #[test]
    fn format_duration_handles_sub_second_and_minutes() {
        assert_eq!(format_duration(Duration::from_millis(500)), "0.500s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m5.000s");
        let mixed = Duration::from_millis(125_750);
        assert_eq!(format_duration(mixed), "2m5.750s");
    }
}
