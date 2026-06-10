use super::*;
use std::time::Duration;

/// Collect ScreenUpdate text until the child completes.
async fn final_screen(cmd: PtyCommand) -> String {
    let (tx, mut rx) = mpsc::channel(64);
    let runner = tokio::spawn(run(cmd, tx));
    let mut last = String::new();
    while let Ok(Some(evt)) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
        match evt {
            ToolEvent::ScreenUpdate { text, .. } => last = text,
            ToolEvent::Complete { .. } | ToolEvent::Fail { .. } => break,
            _ => {}
        }
    }
    let _ = runner.await;
    last
}

/// `GIT_TERMINAL_PROMPT=0` is defaulted onto every PTY child (so git can't
/// hang on a credential prompt), `with_env` entries reach the child, and an
/// explicit `with_env` value overrides the default.
#[tokio::test(flavor = "current_thread")]
async fn env_injection_defaults_prompt_guard_and_allows_override() {
    let probe = "printf '%s\\n' \"$GIT_TERMINAL_PROMPT::$MARKER\"";

    // Default prompt guard + injected MARKER.
    let cmd = PtyCommand::new("env1", "env", "sh")
        .with_args(vec!["-c".into(), probe.into()])
        .with_env(vec![("MARKER".into(), "x".into())]);
    assert!(
        final_screen(cmd).await.contains("0::x"),
        "expected GIT_TERMINAL_PROMPT=0 default and MARKER=x"
    );

    // Explicit env overrides the default guard.
    let cmd = PtyCommand::new("env2", "env", "sh")
        .with_args(vec!["-c".into(), probe.into()])
        .with_env(vec![
            ("GIT_TERMINAL_PROMPT".into(), "9".into()),
            ("MARKER".into(), "y".into()),
        ]);
    assert!(
        final_screen(cmd).await.contains("9::y"),
        "expected with_env to override the prompt-guard default"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runs_echo_under_pty() {
    let (tx, mut rx) = mpsc::channel(32);
    let cmd = PtyCommand::new("p1", "echo", "echo").with_args(vec!["hello-pty".into()]);
    let runner = tokio::spawn(run(cmd, tx));
    let mut saw_start = false;
    let mut saw_screen_with_hello = false;
    let mut saw_complete = false;
    while let Ok(Some(evt)) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
        match evt {
            ToolEvent::Start { .. } => saw_start = true,
            ToolEvent::ScreenUpdate { text, .. } if text.contains("hello-pty") => {
                saw_screen_with_hello = true;
            }
            ToolEvent::Complete { .. } => {
                saw_complete = true;
                break;
            }
            _ => {}
        }
    }
    let _ = runner.await;
    assert!(saw_start, "missing Start");
    assert!(saw_screen_with_hello, "missing 'hello-pty' screen update");
    assert!(saw_complete, "missing Complete");
}

/// Regression: an in-place progress bar (`\r` redraws + clear-line) must
/// collapse onto a single updating line, not accumulate one row per frame.
/// Drives a real progress-bar-shaped stream through the PTY and asserts the
/// final `ScreenUpdate` is the latest frame on one line, with no stale frames
/// leaked. Before the persistent-emulator fix this produced one transcript row
/// per redraw (the 65k-line flood).
#[tokio::test(flavor = "current_thread")]
async fn carriage_return_progress_collapses_to_single_line() {
    let (tx, mut rx) = mpsc::channel(64);
    // `\r` returns to col 0 and ESC[2K clears the line, exactly like
    // indicatif; the final frame is newline-terminated.
    let script = "printf '0/3 scoring\\r\\x1b[2K1/3 scoring\\r\\x1b[2K3/3 done\\n'";
    let cmd = PtyCommand::new("pbar", "progress", "sh").with_args(vec!["-c".into(), script.into()]);
    let runner = tokio::spawn(run(cmd, tx));

    let mut last_screen: Option<String> = None;
    let mut screen_update_count = 0usize;
    while let Ok(Some(evt)) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
        match evt {
            ToolEvent::ScreenUpdate { text, .. } => {
                screen_update_count += 1;
                last_screen = Some(text);
            }
            ToolEvent::Complete { .. } | ToolEvent::Fail { .. } => break,
            _ => {}
        }
    }
    let _ = runner.await;

    let screen = last_screen.expect("expected at least one ScreenUpdate");
    assert!(
        screen.contains("3/3 done"),
        "final frame missing: {screen:?}"
    );
    assert!(!screen.contains("0/3"), "stale frame leaked: {screen:?}");
    assert!(!screen.contains("1/3"), "stale frame leaked: {screen:?}");
    // The progress region collapses to one line regardless of how the OS
    // chunked the bytes across reads.
    assert_eq!(
        screen.lines().filter(|l| !l.trim().is_empty()).count(),
        1,
        "expected a single non-empty line, got: {screen:?}"
    );
    assert!(screen_update_count >= 1);
}

fn which_on_path(prog: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(prog))
        .find(|p| p.is_file())
}

/// End-to-end through the real PTY pipeline: drive an actual `jankurai audit`
/// and assert its in-place progress bar collapses to a *final* screen that
/// shows the `score=` summary - i.e. the live render reflects the last state,
/// not an early/stale frame, and doesn't flood. This is the deterministic e2e
/// for the `/audit` live render: it exercises the same pty_runner + vt100 +
/// ScreenUpdate path the TUI uses, against real audit output, without the flaky
/// keystroke/popup layer. Gated on `jankurai` being on PATH so the lane stays
/// green where it isn't installed.
#[tokio::test(flavor = "current_thread")]
async fn jankurai_audit_renders_final_score_through_pty() {
    let Some(jankurai) = which_on_path("jankurai") else {
        eprintln!("skipped: jankurai not on PATH");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("README.md"), "# audit-render\n").unwrap();
    // Give the scanner a real repo baseline; best-effort.
    let _ = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status();

    let args = vec![
        "audit".into(),
        dir.path().to_string_lossy().into_owned(),
        "--mode".into(),
        "advisory".into(),
        "--json".into(),
        dir.path().join("score.json").to_string_lossy().into_owned(),
        "--md".into(),
        dir.path().join("score.md").to_string_lossy().into_owned(),
    ];
    // `JANKURAI_NO_UPDATE_CHECK=1` mirrors how the TUI spawns `/audit`: a
    // purely local scan, no network round-trip.
    let cmd = PtyCommand::new(
        "aud",
        "jankurai audit",
        jankurai.to_string_lossy().into_owned(),
    )
    .with_args(args)
    .with_env(vec![("JANKURAI_NO_UPDATE_CHECK".into(), "1".into())]);

    let (tx, mut rx) = mpsc::channel(256);
    let runner = tokio::spawn(run(cmd, tx));
    let mut last_screen = String::new();
    let mut outcome = "none";
    while let Ok(Some(evt)) = tokio::time::timeout(Duration::from_secs(60), rx.recv()).await {
        match evt {
            ToolEvent::ScreenUpdate { text, .. } => last_screen = text,
            ToolEvent::Complete { .. } => {
                outcome = "complete";
                break;
            }
            ToolEvent::Fail { error, .. } => {
                outcome = "fail";
                eprintln!("audit Fail: {error}");
                break;
            }
            _ => {}
        }
    }
    let _ = runner.await;

    assert_eq!(outcome, "complete", "audit did not complete cleanly");
    assert!(
        last_screen.contains("score="),
        "final live render should show the score summary; got:\n{last_screen}"
    );
    let lines = last_screen.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(
        lines < 50,
        "render flooded ({lines} non-blank lines):\n{last_screen}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn nonzero_exit_yields_fail() {
    let (tx, mut rx) = mpsc::channel(32);
    let cmd = PtyCommand::new("p2", "false", "false");
    let runner = tokio::spawn(run(cmd, tx));
    let mut saw_fail = false;
    while let Ok(Some(evt)) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
        if matches!(evt, ToolEvent::Fail { .. }) {
            saw_fail = true;
            break;
        }
    }
    let _ = runner.await;
    assert!(saw_fail);
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_kills_long_running() {
    let token = CancellationToken::new();
    let (tx, mut rx) = mpsc::channel(32);
    let cmd = PtyCommand::new("p3", "sleep", "sleep")
        .with_args(vec!["30".into()])
        .with_cancel(token.clone());
    let runner = tokio::spawn(run(cmd, tx));

    tokio::time::sleep(Duration::from_millis(100)).await;
    token.cancel_hard();

    let mut saw_fail = false;
    while let Ok(Some(evt)) = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        if matches!(evt, ToolEvent::Fail { .. }) {
            saw_fail = true;
            break;
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), runner).await;
    assert!(saw_fail, "expected Fail after cancellation");
}
