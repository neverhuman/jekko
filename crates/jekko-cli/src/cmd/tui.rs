//! `jekko tui` — explicit TUI launch (default subcommand).
//!
//! Mirrors the implicit "no subcommand" branch of
//! `packages/jekko/src/index.ts`. The actual frame loop lives in
//! `jekko-tui` (Packets G/H).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::Args;
use jekko_tui::action::JnoccioBootStatus;
use jekko_tui::chat_bridge_backend::{ChatBridgeBackend, ChatBridgeConfig, RecallHook};
use jekko_tui::inline_runtime::{run_inline, InlineRuntimeOptions};

use crate::cli::GlobalOpts;

/// Flags accepted by `jekko tui`. These are the TUI-specific ones; the
/// global flags (--pure, --headless) come in via [`GlobalOpts`].
#[derive(Args, Debug, Default)]
pub struct TuiArgs {
    /// Resume the last session if one exists.
    #[arg(long = "continue", short = 'c')]
    pub r#continue: bool,

    /// Open a specific session by id.
    #[arg(short = 's', value_name = "SESSION_ID")]
    pub session: Option<String>,
}

/// Launch the TUI.
pub fn run(global: &GlobalOpts, args: &TuiArgs) -> Result<()> {
    if let Some(session_id) = args.session.as_deref() {
        eprintln!("note: -s {session_id} not yet wired through TuiOptions");
    }
    if args.r#continue {
        eprintln!("note: --continue not yet wired through TuiOptions");
    }

    // Spawn the Jnoccio boot thread unless explicitly disabled (PTY tests,
    // CI, or environments without jnoccio-fusion configured).
    let jnoccio_rx = spawn_jnoccio_boot_bridge();

    let opts = InlineRuntimeOptions {
        no_alt_screen: global.headless,
        jnoccio_boot_status: if jnoccio_rx.is_some() {
            JnoccioBootStatus::Checking
        } else {
            JnoccioBootStatus::Disabled
        },
        jnoccio_boot_rx: jnoccio_rx,
        ..InlineRuntimeOptions::default()
    };
    let mut backend = ChatBridgeBackend::new(ChatBridgeConfig::default());
    if let Some(hook) = open_tui_memory_hook() {
        backend = backend.with_recall_hook(hook);
    }
    run_inline(backend, opts)
}

type TuiMemoryService = jekko_memory::MemoryService<jekko_store::memory_wal::SqliteWalSink>;

static TUI_MEMORY_RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TuiMemoryHook {
    memory: Arc<Mutex<TuiMemoryService>>,
    run_id: String,
    seq: AtomicU64,
}

impl RecallHook for TuiMemoryHook {
    fn recall_block(&self, prompt: &str) -> Option<String> {
        let mut guard = self.memory.lock().ok()?;
        guard.recall_block_default(prompt)
    }

    fn observe_user(&self, prompt: &str) {
        self.observe("user", prompt);
    }

    fn observe_assistant(&self, text: &str) {
        self.observe("assistant", text);
    }
}

impl TuiMemoryHook {
    fn observe(&self, role: &str, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let turn = jekko_memory::TurnObservation::new(
            format!("tui_{}_{}_{}", self.run_id, role, seq),
            "tui",
            role,
            text,
            now_ms(),
        );
        if let Ok(mut guard) = self.memory.lock() {
            if let Err(err) = guard.observe_turn(turn) {
                eprintln!("warning: jekko tui memory observe failed: {err}");
            }
        }
    }
}

fn open_tui_memory_hook() -> Option<Arc<dyn RecallHook>> {
    match open_tui_memory_hook_inner() {
        Ok(hook) => Some(hook),
        Err(err) => {
            eprintln!("warning: jekko tui memory disabled: {err}");
            None
        }
    }
}

fn open_tui_memory_hook_inner() -> Result<Arc<dyn RecallHook>> {
    let path = tui_memory_db_path().context("JEKKO_DB not set and HOME is unavailable")?;
    if path != Path::new(":memory:") {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create memory db directory {}", parent.display()))?;
        }
    }
    let db = jekko_store::Db::open(&path)
        .with_context(|| format!("open memory db {}", path.display()))?;
    let sink = jekko_store::memory_wal::SqliteWalSink::new(db, "global");
    let mut memory = jekko_memory::MemoryService::new(sink, jekko_memory::MemoryConfig::default());
    memory.bootstrap().context("bootstrap memory WAL")?;
    Ok(Arc::new(TuiMemoryHook {
        memory: Arc::new(Mutex::new(memory)),
        run_id: new_tui_memory_run_id(),
        seq: AtomicU64::new(0),
    }))
}

fn tui_memory_db_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("JEKKO_DB").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(home).join(".jekko").join("jekko.db"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn new_tui_memory_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = TUI_MEMORY_RUN_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    format!("{}_{}_{}", std::process::id(), nanos, counter)
}

/// Spawn the Jnoccio boot thread and return a channel receiver that delivers
/// [`JnoccioBootStatus`] values to the TUI event loop.
///
/// The boot crate uses its own `BootStatus` type; a bridge thread converts
/// the types to keep the TUI crate free of a direct dep on
/// `jekko-jnoccio-boot`.
///
/// Returns `None` when `JEKKO_DISABLE_JNOCCIO_BOOT=1` is set so callers can
/// distinguish "no channel" from "channel active but server unavailable".
fn spawn_jnoccio_boot_bridge() -> Option<std::sync::mpsc::Receiver<JnoccioBootStatus>> {
    if std::env::var("JEKKO_DISABLE_JNOCCIO_BOOT").as_deref() == Ok("1") {
        return None;
    }
    if !jekko_jnoccio_boot::unlock::is_unlocked() {
        return None;
    }

    let (boot_tx, boot_rx) = std::sync::mpsc::channel::<jekko_jnoccio_boot::BootEvent>();
    let (tui_tx, tui_rx) = std::sync::mpsc::channel::<JnoccioBootStatus>();

    // Launch the real boot thread.
    jekko_jnoccio_boot::spawn_boot_thread(boot_tx);

    // Bridge thread: convert BootEvent → JnoccioBootStatus and forward.
    // Exits when the boot thread drops its sender (TUI exit).
    std::thread::Builder::new()
        .name("jnoccio-boot-bridge".into())
        .spawn(move || {
            use jekko_jnoccio_boot::{BootEvent, BootStatus};
            #[allow(clippy::while_let_loop)]
            loop {
                match boot_rx.recv() {
                    Ok(BootEvent::StatusChanged(status)) => {
                        let tui_status = match status {
                            BootStatus::Idle => JnoccioBootStatus::Idle,
                            BootStatus::Checking => JnoccioBootStatus::Checking,
                            BootStatus::Starting => JnoccioBootStatus::Starting,
                            BootStatus::Ready {
                                enabled_models,
                                total_models,
                            } => JnoccioBootStatus::Ready {
                                enabled_models,
                                total_models,
                            },
                            BootStatus::Unavailable => JnoccioBootStatus::NotInstalled,
                            BootStatus::Failed => JnoccioBootStatus::Failed,
                        };
                        if tui_tx.send(tui_status).is_err() {
                            break; // TUI exited
                        }
                    }
                    Err(_) => break, // boot thread gone
                }
            }
        })
        .ok();

    Some(tui_rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        prev_home: Option<std::ffi::OsString>,
        prev_dev: Option<std::ffi::OsString>,
        prev_disable: Option<std::ffi::OsString>,
        prev_jekko_db: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn locked(home: &std::path::Path) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev_home = std::env::var_os("HOME");
            let prev_dev = std::env::var_os("JNOCCIO_DEVELOPER_KEY");
            let prev_disable = std::env::var_os("JEKKO_DISABLE_JNOCCIO_BOOT");
            let prev_jekko_db = std::env::var_os("JEKKO_DB");
            std::env::set_var("HOME", home);
            std::env::remove_var("JNOCCIO_DEVELOPER_KEY");
            std::env::remove_var("JEKKO_DISABLE_JNOCCIO_BOOT");
            std::env::remove_var("JEKKO_DB");
            Self {
                prev_home,
                prev_dev,
                prev_disable,
                prev_jekko_db,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match &self.prev_dev {
                Some(v) => std::env::set_var("JNOCCIO_DEVELOPER_KEY", v),
                None => std::env::remove_var("JNOCCIO_DEVELOPER_KEY"),
            }
            match &self.prev_disable {
                Some(v) => std::env::set_var("JEKKO_DISABLE_JNOCCIO_BOOT", v),
                None => std::env::remove_var("JEKKO_DISABLE_JNOCCIO_BOOT"),
            }
            match &self.prev_jekko_db {
                Some(v) => std::env::set_var("JEKKO_DB", v),
                None => std::env::remove_var("JEKKO_DB"),
            }
        }
    }

    #[test]
    fn jnoccio_boot_bridge_stays_disabled_when_locked() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::locked(home.path());

        assert!(spawn_jnoccio_boot_bridge().is_none());
    }

    #[test]
    fn memory_db_path_uses_jekko_db_env() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::locked(home.path());
        std::env::set_var("JEKKO_DB", "/tmp/custom-jekko.db");

        assert_eq!(
            tui_memory_db_path().unwrap(),
            PathBuf::from("/tmp/custom-jekko.db")
        );
    }

    #[test]
    fn memory_db_path_defaults_under_home() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::locked(home.path());

        assert_eq!(
            tui_memory_db_path().unwrap(),
            home.path().join(".jekko").join("jekko.db")
        );
    }

    #[test]
    fn open_tui_memory_hook_bootstraps_file_db() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::locked(home.path());
        let hook = open_tui_memory_hook_inner().unwrap();

        hook.observe_user("photons are massless");
        assert!(hook.recall_block("photons").is_some());
        assert!(home.path().join(".jekko").join("jekko.db").exists());
    }

    #[test]
    fn tui_memory_hook_uses_restart_safe_event_ids() {
        let home = TempDir::new().unwrap();
        let _guard = EnvGuard::locked(home.path());
        let db_path = home.path().join(".jekko").join("jekko.db");

        let hook = open_tui_memory_hook_inner().unwrap();
        hook.observe_user("photons are massless");
        drop(hook);

        let hook = open_tui_memory_hook_inner().unwrap();
        hook.observe_user("gluons carry color charge");
        drop(hook);

        let db = jekko_store::Db::open(&db_path).unwrap();
        let count: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM cogcore_wal WHERE scope = 'global'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }
}
