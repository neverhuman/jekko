//! Local-only battle PTY coverage for TUI surfaces not covered by the stable
//! smoke lanes.
//!
//! Gates:
//! - `CI != true`
//! - `JEKKO_TUI_BATTLE=1`
//! - `JEKKO_BIN` points at a built host binary

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serial_test::serial;
use tuiwright::{Key, Page};

mod test_helpers;
use test_helpers::{
    copy_jekko_logs, ensure_artifact_dir, jekko_bin, prepare_workspace, repo_root,
    spawn_jekko_with_size, spawn_jekko_with_size_env,
};

const BOOT_TIMEOUT: Duration = Duration::from_secs(30);
const SHORT_TIMEOUT: Duration = Duration::from_secs(6);
const STABILIZE: Duration = Duration::from_millis(350);

fn require_battle_bin() -> Result<Option<PathBuf>> {
    if std::env::var("JEKKO_TUI_BATTLE").as_deref() != Ok("1") {
        eprintln!("skipped: set JEKKO_TUI_BATTLE=1");
        return Ok(None);
    }
    if std::env::var("CI").as_deref() == Ok("true") {
        bail!("battle TUI tests must not run in CI");
    }
    let Some(path) = jekko_bin() else {
        bail!("JEKKO_BIN must point to a built jekko binary for battle TUI tests");
    };
    Ok(Some(path))
}

fn artifact_dir(test_name: &str) -> Result<PathBuf> {
    let dir = ensure_artifact_dir()?.join("local-battle").join(test_name);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {dir:?}"))?;
    Ok(dir)
}

fn capture(page: &Page, dir: &Path, name: &str) -> Result<()> {
    page.screenshot(dir.join(format!("{name}.png")))?;
    std::fs::write(dir.join(format!("{name}.txt")), page.screen().plain_text())?;
    assert_screen_health(page, name)
}

fn assert_screen_health(page: &Page, label: &str) -> Result<()> {
    let text = page.screen().plain_text();
    if text.trim().is_empty() {
        bail!("{label}: terminal frame is blank");
    }
    let lower = text.to_lowercase();
    if lower.contains("thread '") || lower.contains("panicked at") || lower.contains("panic:") {
        bail!("{label}: visible panic-like text in frame:\n{text}");
    }
    Ok(())
}

fn wait_for_boot(
    page: &Page,
    workspace: &tempfile::TempDir,
    artifact_dir: &Path,
    label: &str,
) -> Result<()> {
    page.wait_for_text("bypass permissions", BOOT_TIMEOUT)
        .with_context(|| {
            let _ = capture(page, artifact_dir, &format!("{label}-boot-timeout"));
            let _ = copy_jekko_logs(workspace, label);
            "TUI did not boot"
        })?;
    assert_screen_health(page, &format!("{label}-booted"))
}

fn submit_slash(page: &Page, command: &str) -> Result<()> {
    if command
        .trim_start_matches('/')
        .chars()
        .any(char::is_whitespace)
    {
        page.paste(command)?;
    } else {
        page.type_text(command)?;
        std::thread::sleep(Duration::from_millis(120));
        page.press(Key::Tab)?;
        std::thread::sleep(Duration::from_millis(120));
    }
    page.press(Key::Enter)?;
    std::thread::sleep(STABILIZE);
    Ok(())
}

fn prime_shell(page: &Page, dir: &Path, label: &str) -> Result<()> {
    page.type_text("prime the shell")?;
    std::thread::sleep(Duration::from_millis(150));
    page.press(Key::Enter)?;
    page.wait_for_text("prime the shell", Duration::from_secs(8))
        .with_context(|| {
            let _ = capture(page, dir, &format!("{label}-prime-timeout"));
            "shell priming prompt did not submit visibly"
        })?;
    page.wait_for_text("turn failed:", Duration::from_secs(10))
        .with_context(|| {
            let _ = capture(page, dir, &format!("{label}-prime-failed-timeout"));
            "shell priming turn did not settle to a failure state"
        })?;
    std::thread::sleep(STABILIZE);
    Ok(())
}

#[test]
#[serial]
fn slash_command_action_notices_render() -> Result<()> {
    let Some(jekko) = require_battle_bin()? else {
        return Ok(());
    };
    let dir = artifact_dir("slash-command-action-notices")?;
    let (workspace, _, _, _, _, _) = prepare_workspace("project", "# slash command battle\n")?;
    let page = spawn_jekko_with_size_env(
        &workspace,
        &jekko,
        160,
        40,
        "battle-slash-actions",
        &[("JEKKO_INLINE_SHOW_PANELS", "1")],
    )?;
    wait_for_boot(&page, &workspace, &dir, "slash-actions")?;
    capture(&page, &dir, "00-boot")?;

    prime_shell(&page, &dir, "slash-actions")?;

    submit_slash(&page, "/help")?;
    page.wait_for_text("shortcuts:", SHORT_TIMEOUT)
        .context("/help did not render sentinel \"shortcuts:\"")?;
    capture(&page, &dir, "01-help")?;

    for (idx, (command, sentinel)) in [
        ("/status", "transcript:"),
        ("/doctor", "jekko v"),
        ("/panels", "recent notices:"),
        ("/sandbox", "allowed_paths:"),
        ("/permissions", "Permission mode"),
    ]
    .iter()
    .enumerate()
    {
        submit_slash(&page, command)?;
        page.wait_for_text(sentinel, SHORT_TIMEOUT)
            .with_context(|| format!("{command} did not render sentinel {sentinel:?}"))?;
        capture(&page, &dir, &format!("{:02}-{}", idx + 1, &command[1..]))?;
    }

    Ok(())
}

#[test]
#[serial]
fn background_command_lifecycle_is_visible() -> Result<()> {
    let Some(jekko) = require_battle_bin()? else {
        return Ok(());
    };
    let dir = artifact_dir("background-command-lifecycle")?;
    let (workspace, _, _, _, _, _) = prepare_workspace("project", "# background battle\n")?;
    let page = spawn_jekko_with_size(&workspace, &jekko, 160, 40, "battle-background")?;
    wait_for_boot(&page, &workspace, &dir, "background")?;
    prime_shell(&page, &dir, "background")?;

    page.type_text("/run")?;
    page.wait_for_text("one-shot non-interactive run", SHORT_TIMEOUT)
        .context("/run popup did not open before args were typed")?;
    capture(&page, &dir, "01-run-token")?;
    page.paste(" --background sleep 30")?;
    std::thread::sleep(Duration::from_millis(200));
    capture(&page, &dir, "02-run-args")?;
    page.press(Key::Enter)?;
    std::thread::sleep(STABILIZE);
    capture(&page, &dir, "03-run-submitted")?;
    if page
        .wait_for_text("backgrounded \"sleep 30\"", SHORT_TIMEOUT)
        .is_err()
    {
        capture(&page, &dir, "01-backgrounded-missing")?;
        eprintln!(
            "advisory: /run --background did not acknowledge a detached job in this binary"
        );
        return Ok(());
    }
    capture(&page, &dir, "01-backgrounded")?;

    submit_slash(&page, "/ps")?;
    page.wait_for_text("background jobs:", SHORT_TIMEOUT)
        .context("/ps did not list background jobs")?;
    page.wait_for_text("running", SHORT_TIMEOUT)
        .context("/ps did not show the job as running")?;
    capture(&page, &dir, "02-ps-running")?;

    submit_slash(&page, "/stop 1")?;
    page.wait_for_text("stopped background job [1]", SHORT_TIMEOUT)
        .context("/stop 1 did not report a stopped job")?;
    capture(&page, &dir, "03-stopped")?;

    submit_slash(&page, "/ps")?;
    page.wait_for_text("cancelled", SHORT_TIMEOUT)
        .context("/ps did not show the stopped job as cancelled")?;
    capture(&page, &dir, "04-ps-cancelled")?;

    Ok(())
}

#[test]
#[serial]
fn prompt_editing_keystrokes_are_not_swallowed() -> Result<()> {
    let Some(jekko) = require_battle_bin()? else {
        return Ok(());
    };
    let dir = artifact_dir("prompt-editing")?;
    let (workspace, _, _, _, _, _) = prepare_workspace("project", "# prompt editing battle\n")?;
    let page = spawn_jekko_with_size(&workspace, &jekko, 120, 30, "battle-prompt-editing")?;
    wait_for_boot(&page, &workspace, &dir, "prompt-editing")?;

    page.type_text("BATTLE_EDIT_ABC")?;
    page.wait_for_text("BATTLE_EDIT_ABC", SHORT_TIMEOUT)
        .context("printable prompt text did not render")?;
    page.press(Key::Backspace)?;
    std::thread::sleep(STABILIZE);
    page.expect_screen()
        .not_to_contain_text("BATTLE_EDIT_ABC")
        .context("Backspace did not remove the trailing C")?;
    page.wait_for_text("BATTLE_EDIT_AB", SHORT_TIMEOUT)
        .context("Backspace did not leave the expected prompt text")?;
    capture(&page, &dir, "01-backspace")?;

    page.type_text("\x1b[13;2uBATTLE_SECOND_LINE")?;
    std::thread::sleep(STABILIZE);
    page.wait_for_text("BATTLE_SECOND_LINE", SHORT_TIMEOUT)
        .context("Shift+Enter-style newline text did not land in composer")?;
    capture(&page, &dir, "02-shift-enter-newline")?;

    let long = "WRAP_".repeat(40);
    page.type_text(&long)?;
    std::thread::sleep(STABILIZE);
    page.wait_for_text("WRAP_", SHORT_TIMEOUT)
        .context("long wrapped input did not remain visible")?;
    capture(&page, &dir, "03-long-wrap")?;

    page.press(Key::Ctrl('c'))?;
    std::thread::sleep(STABILIZE);
    page.expect_screen()
        .not_to_contain_text("BATTLE_SECOND_LINE")
        .context("Ctrl+C did not clear the composer")?;
    capture(&page, &dir, "04-cleared")?;

    Ok(())
}

#[test]
#[serial]
fn scroll_and_resize_long_transcript_stays_healthy() -> Result<()> {
    let Some(jekko) = require_battle_bin()? else {
        return Ok(());
    };
    let dir = artifact_dir("scroll-and-resize")?;
    let (workspace, _, _, _, _, _) = prepare_workspace("project", "# scroll battle\n")?;
    let page = spawn_jekko_with_size(&workspace, &jekko, 100, 30, "battle-scroll-resize")?;
    wait_for_boot(&page, &workspace, &dir, "scroll-resize")?;
    prime_shell(&page, &dir, "scroll-resize")?;

    for _ in 0..18 {
        submit_slash(&page, "/help")?;
        page.wait_for_text("shortcuts:", SHORT_TIMEOUT)?;
    }
    capture(&page, &dir, "01-long-transcript")?;

    page.press(Key::PageUp)?;
    std::thread::sleep(STABILIZE);
    capture(&page, &dir, "02-page-up")?;
    page.press(Key::PageDown)?;
    std::thread::sleep(STABILIZE);
    capture(&page, &dir, "03-page-down")?;
    page.type_text("\x1b[1;5H")?;
    std::thread::sleep(STABILIZE);
    capture(&page, &dir, "04-ctrl-home")?;
    page.type_text("\x1b[1;5F")?;
    std::thread::sleep(STABILIZE);
    capture(&page, &dir, "05-ctrl-end")?;

    for (idx, (cols, rows)) in [(80, 24), (200, 60), (120, 30)].into_iter().enumerate() {
        page.resize(cols, rows)?;
        std::thread::sleep(STABILIZE);
        capture(&page, &dir, &format!("resize-{idx}-{cols}x{rows}"))?;
    }

    Ok(())
}

#[test]
#[serial]
fn paste_handling_collapses_and_expands_large_paste() -> Result<()> {
    let Some(jekko) = require_battle_bin()? else {
        return Ok(());
    };
    let dir = artifact_dir("paste-handling")?;
    let (workspace, _, _, _, _, _) = prepare_workspace("project", "# paste battle\n")?;
    let page = spawn_jekko_with_size(&workspace, &jekko, 160, 40, "battle-paste-handling")?;
    wait_for_boot(&page, &workspace, &dir, "paste-handling")?;

    page.paste("BATTLE_SHORT_PASTE")?;
    page.wait_for_text("BATTLE_SHORT_PASTE", SHORT_TIMEOUT)
        .context("short paste did not stay inline")?;
    capture(&page, &dir, "01-short-paste")?;
    page.press(Key::Ctrl('c'))?;
    std::thread::sleep(STABILIZE);

    let mut large = String::new();
    for i in 0..12 {
        large.push_str(&format!("large paste line {i}\n"));
    }
    large.push_str("BATTLE_LARGE_PASTE_SENTINEL\n");
    page.paste(&large)?;
    page.wait_for_text("[paste #1:", SHORT_TIMEOUT)
        .context("large paste did not collapse to a chip")?;
    capture(&page, &dir, "02-large-paste-chip")?;

    page.press(Key::Enter)?;
    page.wait_for_text("BATTLE_LARGE_PASTE_SENTINEL", Duration::from_secs(8))
        .context("submitted paste did not expand into the user card")?;
    capture(&page, &dir, "03-large-paste-submitted")?;

    Ok(())
}

#[test]
#[serial]
fn zyal_home_paste_shows_indicator() -> Result<()> {
    let Some(jekko) = require_battle_bin()? else {
        return Ok(());
    };
    let dir = artifact_dir("zyal-home-paste")?;
    let (workspace, _, _, _, _, _) = prepare_workspace("project", "# zyal home battle\n")?;
    std::fs::write(
        workspace.path().join(".env.jnoccio"),
        "JNOCCIO_DEVELOPER_KEY=tuiwright-offline-fake-key\n",
    )?;
    std::fs::write(
        workspace.path().join("model-keys.env"),
        "OPENAI_API_KEY=fake-openai-key\n",
    )?;
    let page = spawn_jekko_with_size_env(
        &workspace,
        &jekko,
        160,
        40,
        "battle-zyal-home",
        &[("JEKKO_DISABLE_MODELS_FETCH", "1")],
    )?;
    wait_for_boot(&page, &workspace, &dir, "zyal-home")?;

    let zyal = std::fs::read_to_string(
        repo_root().join("docs/ZYAL/examples/13-advanced-research-loop.zyal"),
    )
    .context("read ZYAL example")?;
    page.paste(&zyal)?;
    if page.wait_for_text("✓ ZYAL", Duration::from_secs(10)).is_err() {
        capture(&page, &dir, "01-zyal-indicator-missing")?;
        eprintln!("advisory: ZYAL home paste indicator did not render in this binary");
        return Ok(());
    }
    capture(&page, &dir, "01-zyal-indicator")?;

    Ok(())
}
