use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use zyalc::{compile, live_audit, replay_verify, runbook_lint};

#[derive(Parser, Debug)]
#[command(
    name = "zyalc",
    version,
    about = "Compile .zyal source files to TOML, GitHub Actions YAML, or SuperWorkflow JSON"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Compile a single .zyal file or all known sources.
    Compile {
        /// Path to a .zyal file. Omit (with --all) to compile every registered source.
        path: Option<PathBuf>,
        /// Override the output path (single-file mode only).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Compile every registered .zyal source under the repo (uses agent/generated-zones.toml).
        #[arg(long)]
        all: bool,
        /// Verify the existing target matches a freshly-compiled output; exit 1 if drifted.
        #[arg(long)]
        check: bool,
        /// Repo root (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Print the detected profile + emitted target for a `.zyal` file.
    Inspect { path: PathBuf },
    /// Lint strict superreasoning runbooks.
    LintSuper {
        /// Path to a .zyal file. Omit with --all to lint known sources.
        path: Option<PathBuf>,
        /// Lint every known superreasoning runbook.
        #[arg(long)]
        all: bool,
        /// Enable strict completion-gate checks.
        #[arg(long)]
        strict: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
        /// Repo root (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Independently verify a completed superreasoning run directory.
    VerifyReplay {
        /// Path to the run directory containing superreasoning_packet.json
        /// and replay_receipt.json.
        run_dir: PathBuf,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
        /// Exit non-zero on the first failure rather than reporting all.
        #[arg(long)]
        strict: bool,
    },
    /// Audit a completed live superreasoning run for credential provenance
    /// and non-substituted live model outcomes.
    AuditLiveRun {
        /// Path to the run directory containing superreasoning artifacts.
        run_dir: PathBuf,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
        /// Fail on any missing or substituted live evidence.
        #[arg(long)]
        strict: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match dispatch(&cli) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("zyalc: {err:#}");
            1
        }
    };
    std::process::exit(code);
}

fn dispatch(cli: &Cli) -> Result<i32> {
    match &cli.cmd {
        Cmd::Compile {
            path,
            out,
            all,
            check,
            root,
        } => {
            if *all {
                let report = compile::compile_all(root, *check)
                    .context("compile --all from generated-zones.toml")?;
                if report.drifted.is_empty() {
                    println!(
                        "zyalc: {} compiled, {} unchanged",
                        report.compiled.len(),
                        report.unchanged.len()
                    );
                    Ok(0)
                } else {
                    eprintln!(
                        "zyalc: drift detected in {} target(s):",
                        report.drifted.len()
                    );
                    for path in &report.drifted {
                        eprintln!("  - {}", path.display());
                    }
                    Ok(1)
                }
            } else {
                let path = path
                    .as_ref()
                    .context("provide a .zyal path or pass --all")?;
                let outcome = compile::compile_one(path, out.as_deref(), *check)?;
                match outcome {
                    compile::Outcome::Wrote(p) => {
                        println!("zyalc: wrote {}", p.display());
                        Ok(0)
                    }
                    compile::Outcome::Unchanged(p) => {
                        println!("zyalc: unchanged {}", p.display());
                        Ok(0)
                    }
                    compile::Outcome::Drift(p) => {
                        eprintln!("zyalc: drift detected in {}", p.display());
                        Ok(1)
                    }
                }
            }
        }
        Cmd::Inspect { path } => {
            let info = compile::inspect(path)?;
            println!("profile: {}", info.profile);
            if let Some(target) = info.target {
                println!("target:  {}", target.display());
            }
            if let Some(schema) = info.schema {
                println!("schema:  {schema}");
            }
            Ok(0)
        }
        Cmd::LintSuper {
            path,
            all,
            strict,
            format,
            root,
        } => {
            let report = if *all {
                runbook_lint::lint_all(root, *strict)?
            } else {
                let path = path
                    .as_ref()
                    .context("provide a .zyal path or pass --all")?;
                runbook_lint::lint_file(path, *strict)?
            };
            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if report.findings.is_empty() {
                println!(
                    "zyalc lint-super: {} checked, 0 findings",
                    report.checked.len()
                );
            } else {
                for finding in &report.findings {
                    eprintln!(
                        "{}: {}: {}",
                        finding.path.display(),
                        finding.code,
                        finding.message
                    );
                }
            }
            Ok(if report.findings.is_empty() { 0 } else { 1 })
        }
        Cmd::VerifyReplay {
            run_dir,
            format,
            strict,
        } => {
            let report = if *strict {
                replay_verify::verify_strict(run_dir)?
            } else {
                replay_verify::verify(run_dir)?
            };
            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if report.status == "passed" {
                println!(
                    "zyalc verify-replay: passed — {} artifact(s), {} gate(s)",
                    report.artifact_count,
                    report.gates.len()
                );
            } else {
                eprintln!("zyalc verify-replay: failed");
                for failure in &report.failures {
                    eprintln!("  - {failure}");
                }
            }
            Ok(report.exit_code())
        }
        Cmd::AuditLiveRun {
            run_dir,
            format,
            strict,
        } => {
            let report = live_audit::audit(run_dir, *strict)?;
            live_audit::print_report(&report, format)?;
            if *strict && report.status != "passed" {
                anyhow::bail!("live audit failed: {}", report.failures.join("; "));
            }
            Ok(report.exit_code())
        }
    }
}
