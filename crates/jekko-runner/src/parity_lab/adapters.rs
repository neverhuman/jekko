//! Reference/candidate adapter implementations for target-switched parity.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result};

use crate::hashing::sha256_hex;

use super::helpers::perf_payload;
use super::types::{ParityCase, ParityResult};

/// Target adapter for reference/candidate switched execution.
pub trait TargetAdapter {
    /// Adapter name.
    fn name(&self) -> &str;
    /// Setup before cases run.
    fn setup(&mut self) -> Result<()>;
    /// Run one case.
    fn run_case(&mut self, case: &ParityCase) -> Result<ParityResult>;
}

/// Shell command adapter. Each parity step is sent to the command stdin and
/// stdout is compared against the expected text.
#[derive(Debug, Clone)]
pub struct CommandTargetAdapter {
    name: String,
    command: String,
    cwd: PathBuf,
}

impl CommandTargetAdapter {
    /// Construct a command adapter.
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            cwd: cwd.into(),
        }
    }
}

impl TargetAdapter for CommandTargetAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    fn run_case(&mut self, case: &ParityCase) -> Result<ParityResult> {
        let started = Instant::now();
        let mut last_stdout = Vec::new();
        let mut last_stderr = Vec::new();
        let mut last_exit_code = Some(0);
        for step in &case.steps {
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(&self.command)
                .current_dir(&self.cwd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| format!("spawn parity command `{}`", self.command))?;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(step.send.as_bytes())?;
            }
            let output = child.wait_with_output()?;
            last_stdout = output.stdout.clone();
            last_stderr = output.stderr.clone();
            last_exit_code = output.status.code();
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !output.status.success() {
                let elapsed = started.elapsed();
                return Ok(ParityResult {
                    case_id: case.id.clone(),
                    target: self.name.clone(),
                    status: "failed".to_string(),
                    skipped: false,
                    message: Some(String::from_utf8_lossy(&output.stderr).to_string()),
                    perf: perf_payload(case, elapsed.as_millis() as u64, elapsed.as_nanos()),
                    stdout_sha256: Some(sha256_hex(&output.stdout)),
                    stderr_sha256: Some(sha256_hex(&output.stderr)),
                    exit_code: output.status.code(),
                    elapsed_nanos: Some(elapsed.as_nanos()),
                    latency_ratio: None,
                    artifact_dir: None,
                    diagnostics: Some(serde_json::json!({"reason": "nonzero_exit"})),
                });
            }
            if stdout.trim_end() != step.expect.trim_end() {
                let elapsed = started.elapsed();
                return Ok(ParityResult {
                    case_id: case.id.clone(),
                    target: self.name.clone(),
                    status: "failed".to_string(),
                    skipped: false,
                    message: Some(format!("expected {:?}, got {:?}", step.expect, stdout)),
                    perf: perf_payload(case, elapsed.as_millis() as u64, elapsed.as_nanos()),
                    stdout_sha256: Some(sha256_hex(&output.stdout)),
                    stderr_sha256: Some(sha256_hex(&output.stderr)),
                    exit_code: output.status.code(),
                    elapsed_nanos: Some(elapsed.as_nanos()),
                    latency_ratio: None,
                    artifact_dir: None,
                    diagnostics: Some(serde_json::json!({"reason": "stdout_mismatch"})),
                });
            }
        }
        let elapsed = started.elapsed();
        Ok(ParityResult {
            case_id: case.id.clone(),
            target: self.name.clone(),
            status: "passed".to_string(),
            skipped: false,
            message: None,
            perf: perf_payload(case, elapsed.as_millis() as u64, elapsed.as_nanos()),
            stdout_sha256: Some(sha256_hex(&last_stdout)),
            stderr_sha256: Some(sha256_hex(&last_stderr)),
            exit_code: last_exit_code,
            elapsed_nanos: Some(elapsed.as_nanos()),
            latency_ratio: None,
            artifact_dir: None,
            diagnostics: None,
        })
    }
}

/// Tiny fake adapter for deterministic smoke tests.
#[derive(Debug, Default)]
pub struct FakeTargetAdapter {
    name: String,
    fail_case_id: Option<String>,
}

impl FakeTargetAdapter {
    /// Construct a fake adapter.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fail_case_id: None,
        }
    }

    /// Configure one case id to fail.
    pub fn fail_case(mut self, case_id: impl Into<String>) -> Self {
        self.fail_case_id = Some(case_id.into());
        self
    }
}

impl TargetAdapter for FakeTargetAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    fn run_case(&mut self, case: &ParityCase) -> Result<ParityResult> {
        let failed = self.fail_case_id.as_deref() == Some(case.id.as_str());
        Ok(ParityResult {
            case_id: case.id.clone(),
            target: self.name.clone(),
            status: if failed { "failed" } else { "passed" }.to_string(),
            skipped: false,
            message: if failed {
                Some("fake failure".into())
            } else {
                None
            },
            perf: case
                .requires_perf()
                .then(|| serde_json::json!({"p95_ms": 1.0, "elapsed_nanos": 1_000_000})),
            stdout_sha256: Some(sha256_hex(b"fake")),
            stderr_sha256: Some(sha256_hex(b"")),
            exit_code: Some(if failed { 1 } else { 0 }),
            elapsed_nanos: Some(1_000_000),
            latency_ratio: None,
            artifact_dir: None,
            diagnostics: None,
        })
    }
}
