use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::hashing::sha256_hex;

use super::{ToolAdapter, ToolDescriptor, ToolKind, ToolLease, ToolOutput};

/// Execution limits + interpreter for a sealed `code.exec` run.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    /// Interpreter binary (`python3`/`node`/`sh`), resolved on the sealed PATH.
    pub interpreter: String,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            interpreter: "python3".to_string(),
            timeout_ms: 5_000,
            max_output_bytes: 64 * 1024,
        }
    }
}

impl SandboxPolicy {
    /// Read optional `interpreter`/`timeout_ms`/`max_output_bytes` overrides from
    /// the tool input (clamped to sane bounds), else defaults.
    pub fn from_input(input: &Value) -> Self {
        let d = Self::default();
        SandboxPolicy {
            interpreter: input
                .get("interpreter")
                .and_then(Value::as_str)
                .unwrap_or(&d.interpreter)
                .to_string(),
            timeout_ms: input
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(d.timeout_ms)
                .clamp(50, 60_000),
            max_output_bytes: input
                .get("max_output_bytes")
                .and_then(Value::as_u64)
                .map(|v| v as usize)
                .unwrap_or(d.max_output_bytes)
                .clamp(256, 4 * 1024 * 1024),
        }
    }
}

/// The minimal environment a sealed sandbox runs with — NO inherited secrets,
/// only a safe PATH so the interpreter resolves, blanked proxy vars so the code
/// cannot egress via the environment, and a blank HOME. (True network isolation
/// needs OS namespaces; the kernel's capability gate enforces that `network.fetch`
/// is never granted to a sealed `code.exec` tool.)
fn sealed_env() -> Vec<(&'static str, String)> {
    vec![
        (
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string(),
        ),
        ("HTTP_PROXY", String::new()),
        ("HTTPS_PROXY", String::new()),
        ("http_proxy", String::new()),
        ("https_proxy", String::new()),
        ("ALL_PROXY", String::new()),
        ("NO_PROXY", "*".to_string()),
        ("HOME", String::new()),
    ]
}

/// Truncate output to `max` bytes on a char boundary, annotating the cut.
pub(crate) fn cap_output(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated {} bytes]", &s[..end], s.len() - end)
}

static SBX_SEQ: AtomicU64 = AtomicU64::new(0);

/// Run `src` in a sealed subprocess: cleared env, scoped temp cwd, wall-clock
/// timeout (killed on overrun), and output read CONCURRENTLY + capped (so a noisy
/// program can't deadlock on a full pipe). Pure of secrets — the inherited
/// environment is dropped entirely (`env_clear`).
///
/// Network: the proxy env is blanked AND `authorize_tool_call` denies
/// `network.fetch` to a `sealed` tool, so a sealed run has no granted egress.
/// Full kernel-level isolation (blocking direct sockets) still requires OS
/// namespaces — out of scope here.
///
/// Limitation: `child.kill()` on timeout signals only the direct child. A script
/// that spawns BACKGROUND grandchildren (`sh -c 'task & exit'`) can orphan them;
/// bounding those requires OS process-group signaling / namespaces.
pub fn run_sealed(src: &str, policy: &SandboxPolicy) -> Result<ToolOutput, String> {
    let start = Instant::now();
    let cwd = std::env::temp_dir().join(format!(
        "jekko-sbx-{}-{}",
        std::process::id(),
        SBX_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&cwd).map_err(|e| format!("sandbox cwd: {e}"))?;

    let mut child = match Command::new(&policy.interpreter)
        .arg("-c")
        .arg(src)
        .env_clear()
        .envs(sealed_env())
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_dir_all(&cwd);
            return Err(format!("sandbox spawn `{}`: {e}", policy.interpreter));
        }
    };

    // Drain stdout/stderr concurrently, capped at max+1 bytes, so a full pipe
    // never deadlocks the timeout loop.
    let max = policy.max_output_bytes;
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut p) = out_pipe {
            let _ = (&mut p).take((max + 1) as u64).read_to_string(&mut s);
        }
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut p) = err_pipe {
            let _ = (&mut p).take((max + 1) as u64).read_to_string(&mut s);
        }
        s
    });

    let deadline = start + Duration::from_millis(policy.timeout_ms);
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = fs::remove_dir_all(&cwd);
                return Err(format!("sandbox wait: {e}"));
            }
        }
    }

    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let _ = fs::remove_dir_all(&cwd);

    if timed_out {
        return Err(format!("sandbox timeout after {}ms", policy.timeout_ms));
    }
    let mut combined = stdout;
    if !stderr.trim().is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str("[stderr] ");
        combined.push_str(stderr.trim_end());
    }
    Ok(ToolOutput {
        output: cap_output(&combined, max),
        cost_usd: 0.0,
        latency_ms: start.elapsed().as_millis() as u64,
    })
}

/// `code.exec` — a sealed, no-network sandbox. In FAKE mode (CI / replay) it
/// returns a deterministic digest of the source without spawning a process; when
/// live it runs [`run_sealed`] (cleared env, scoped cwd, timeout, capped output).
pub struct CodeExecAdapter {
    pub tool_id: String,
    pub fake: bool,
}

impl CodeExecAdapter {
    /// FAKE mode is on under test, when `JEKKO_TOOLS_FAKE=1`, or when asked.
    pub fn new(tool_id: impl Into<String>, fake: bool) -> Self {
        let fake = fake
            || cfg!(test)
            || std::env::var("JEKKO_TOOLS_FAKE")
                .map(|v| v == "1")
                .unwrap_or(false);
        CodeExecAdapter {
            tool_id: tool_id.into(),
            fake,
        }
    }
}

impl ToolAdapter for CodeExecAdapter {
    fn tool_id(&self) -> &str {
        &self.tool_id
    }

    fn invoke(&self, _lease: &ToolLease, input: &Value) -> Result<ToolOutput, String> {
        let src = input.get("src").and_then(Value::as_str).unwrap_or("");
        if self.fake {
            let digest = &sha256_hex(src.as_bytes())[..12];
            return Ok(ToolOutput {
                output: format!("fake-exec:{digest}"),
                cost_usd: 0.0,
                latency_ms: 0,
            });
        }
        run_sealed(src, &SandboxPolicy::from_input(input))
    }
}

// ---- theorem.prover + workflow.call adapters (M9-cont) ---------------------

/// `theorem.prover` — a deterministic STRUCTURAL proof checker (NOT a logical
/// solver). Input: `{goal, proof:{steps:[{justification, conclusion}, ...]}}`. A
/// proof is accepted iff it has ≥1 step, every step carries a non-empty
/// `justification`, and the final step's `conclusion` equals `goal`. A missing/
/// empty `goal` always yields `unproved` (a non-empty goal is required). This
/// validates proof SHAPE, not soundness — `proved` means "well-formed against the
/// declared goal", which the runner turns into a `proves` edge; a real logical
/// checker is a future adapter behind the same interface.
pub struct TheoremProverAdapter {
    pub tool_id: String,
}

impl TheoremProverAdapter {
    pub fn new(tool_id: impl Into<String>) -> Self {
        Self {
            tool_id: tool_id.into(),
        }
    }
}

impl ToolAdapter for TheoremProverAdapter {
    fn tool_id(&self) -> &str {
        &self.tool_id
    }

    fn invoke(&self, _lease: &ToolLease, input: &Value) -> Result<ToolOutput, String> {
        let goal = input.get("goal").and_then(Value::as_str).unwrap_or("");
        let steps = input
            .get("proof")
            .and_then(|p| p.get("steps"))
            .and_then(Value::as_array);
        let (status, reason) = check_proof(goal, steps);
        Ok(ToolOutput {
            output: serde_json::json!({ "proof_status": status, "reason": reason }).to_string(),
            cost_usd: 0.0,
            latency_ms: 0,
        })
    }
}

/// The structural proof check used by [`TheoremProverAdapter`]. Pure.
fn check_proof(goal: &str, steps: Option<&Vec<Value>>) -> (&'static str, String) {
    let Some(steps) = steps else {
        return ("unproved", "no proof.steps".to_string());
    };
    if steps.is_empty() {
        return ("unproved", "empty proof".to_string());
    }
    for (i, s) in steps.iter().enumerate() {
        let j = s.get("justification").and_then(Value::as_str).unwrap_or("");
        if j.trim().is_empty() {
            return ("unproved", format!("step {i} missing justification"));
        }
    }
    let last = steps
        .last()
        .and_then(|s| s.get("conclusion"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if !goal.is_empty() && last == goal {
        ("proved", "final conclusion matches goal".to_string())
    } else {
        (
            "unproved",
            "final conclusion does not match goal".to_string(),
        )
    }
}

/// `workflow.call` — invokes a child workflow as a BUDGETED tool. Input:
/// `{workflow, args?, budget_usd}`. Deterministic: returns a content digest of
/// (workflow, args) as the child result + a flat per-call cost; a non-positive
/// budget is refused (the parent must allocate budget before delegating).
pub struct WorkflowCallAdapter {
    pub tool_id: String,
    /// Flat modeled cost charged per child call.
    pub per_call_usd: f64,
}

impl WorkflowCallAdapter {
    pub fn new(tool_id: impl Into<String>) -> Self {
        Self {
            tool_id: tool_id.into(),
            per_call_usd: 0.01,
        }
    }
}

impl ToolAdapter for WorkflowCallAdapter {
    fn tool_id(&self) -> &str {
        &self.tool_id
    }

    fn invoke(&self, _lease: &ToolLease, input: &Value) -> Result<ToolOutput, String> {
        let workflow = input.get("workflow").and_then(Value::as_str).unwrap_or("");
        if workflow.is_empty() {
            return Err("workflow.call: missing `workflow`".to_string());
        }
        let budget = input
            .get("budget_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if !budget.is_finite() || budget <= 0.0 {
            return Err("workflow.call: a positive `budget_usd` must be allocated".to_string());
        }
        if self.per_call_usd > budget {
            return Err(format!(
                "workflow.call: child cost {:.4} exceeds budget {:.4}",
                self.per_call_usd, budget
            ));
        }
        let args = input.get("args").cloned().unwrap_or(Value::Null);
        let digest = &sha256_hex(format!("{workflow}|{args}").as_bytes())[..12];
        Ok(ToolOutput {
            output:
                serde_json::json!({ "child": workflow, "result": format!("child-result:{digest}") })
                    .to_string(),
            cost_usd: self.per_call_usd,
            latency_ms: 0,
        })
    }
}

/// Resolve the executable adapter for a tool descriptor — the kernel's dispatch
/// seam (M9-cont). The run loop looks up a descriptor in the [`ToolRegistry`] and
/// calls [`run_tool`] with `adapter_for(descriptor)`. New tool kinds are wired
/// here; `code.exec` runs the sealed sandbox (FAKE under CI/test).
///
/// An unrecognized kind falls back to a sealed FAKE code adapter — FAIL-SAFE
/// (never live I/O), but note the trade-off: a misconfigured/unimplemented tool
/// silently returns a fake digest instead of erroring, so a tool that should run
/// won't (and won't surface a failure). Mcp/Http/Shell/Plugin adapters land in
/// the roadmap tail; until then this is the deliberate fail-safe default.
pub fn adapter_for(descriptor: &ToolDescriptor) -> Box<dyn ToolAdapter> {
    let id = descriptor.tool_id.clone();
    match descriptor.kind {
        ToolKind::Code => Box::new(CodeExecAdapter::new(id, false)),
        ToolKind::Workflow => Box::new(WorkflowCallAdapter::new(id)),
        _ if descriptor.tool_id == "theorem.prover" => Box::new(TheoremProverAdapter::new(id)),
        _ => Box::new(CodeExecAdapter::new(id, true)),
    }
}
