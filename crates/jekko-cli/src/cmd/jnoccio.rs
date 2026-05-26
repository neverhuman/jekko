//! `jekko jnoccio` local unlock and status commands.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::json;

use crate::cli::GlobalOpts;

/// Jnoccio Fusion command group.
#[derive(Args, Debug)]
pub struct JnoccioArgs {
    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: JnoccioCommand,
}

/// Jnoccio Fusion subcommands.
#[derive(Subcommand, Debug)]
pub enum JnoccioCommand {
    /// Show local Jnoccio unlock, checkout, and health status.
    Status(StatusArgs),
    /// Unlock a Jnoccio checkout from a 128-character software secret.
    Unlock(UnlockArgs),
}

/// Arguments for `jekko jnoccio status`.
#[derive(Args, Debug, Default)]
pub struct StatusArgs {
    /// Repository root containing `jnoccio-fusion/`.
    #[arg(long = "repo-root", value_name = "PATH")]
    pub repo_root: Option<PathBuf>,
}

/// Arguments for `jekko jnoccio unlock`.
#[derive(Args, Debug)]
pub struct UnlockArgs {
    /// Repository root containing `jnoccio-fusion/`.
    #[arg(long = "repo-root", value_name = "PATH")]
    pub repo_root: Option<PathBuf>,

    /// 128-character software unlock secret path.
    #[arg(long = "secret-path", value_name = "PATH")]
    pub secret_path: Option<PathBuf>,

    /// Provider-key env file copied into `jnoccio-fusion/.env.jnoccio` when missing.
    #[arg(long = "env-source", value_name = "PATH")]
    pub env_source: Option<PathBuf>,

    /// User id whose `llm.env` receives `JNOCCIO_DEVELOPER_KEY`.
    ///
    /// When omitted, every existing `~/.jekko/users/*/llm.env` receives it so
    /// the live runtime can balance Jnoccio calls across all user folders.
    #[arg(long = "user")]
    pub user: Option<String>,

    /// Do not install `JNOCCIO_DEVELOPER_KEY` into home/user key files.
    #[arg(long = "no-install-developer-key")]
    pub no_install_developer_key: bool,

    /// Overwrite an existing `jnoccio-fusion/.env.jnoccio` from `--env-source`.
    #[arg(long = "force-env-copy")]
    pub force_env_copy: bool,

    /// Refresh tracked `jnoccio-fusion/` files from the index after installing the key.
    ///
    /// This is useful when the checkout still contains ciphertext blobs, but it
    /// overwrites local changes under `jnoccio-fusion/`.
    #[arg(long = "force-checkout-refresh")]
    pub force_checkout_refresh: bool,
}

/// Run the Jnoccio command group.
pub fn run(_global: &GlobalOpts, args: &JnoccioArgs) -> Result<()> {
    match &args.command {
        JnoccioCommand::Status(status) => status_command(status),
        JnoccioCommand::Unlock(unlock) => unlock_command(unlock),
    }
}

fn status_command(args: &StatusArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root.as_deref())?;
    let health = jekko_jnoccio_boot::health::probe_health();
    let home_env = home_env_path();
    let secret_path = default_secret_path();
    let body = json!({
        "repo_root": repo_root,
        "developer_unlocked": jekko_jnoccio_boot::unlock::is_unlocked(),
        "plaintext_checkout": jekko_jnoccio_boot::unlock::has_plaintext_signals(&repo_root),
        "configured": jekko_jnoccio_boot::unlock::is_configured(&repo_root),
        "secret_path_present": secret_path.is_file(),
        "home_env_has_developer_key": env_file_has_key(&home_env, "JNOCCIO_DEVELOPER_KEY"),
        "jnoccio_user_key_users": user_ids_with_key("JNOCCIO_DEVELOPER_KEY"),
        "health": {
            "reachable": health.reachable,
            "enabled_models": health.enabled_models,
            "total_models": health.total_models,
        },
    });
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

fn unlock_command(args: &UnlockArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root.as_deref())?;
    let secret_path = match &args.secret_path {
        Some(path) => path.clone(),
        None => default_secret_path(),
    };
    let secret_path = expand_home_path(&secret_path);
    let secret = jekko_jnoccio_boot::secret_unlock::read_unlock_secret(&secret_path)?;

    let unlock_report = jekko_jnoccio_boot::secret_unlock::unlock_repo_with_secret_options(
        &repo_root,
        &secret,
        args.force_checkout_refresh,
    )
    .with_context(|| format!("unlock {}", repo_root.display()))?;

    let env_source = match &args.env_source {
        Some(path) => path.clone(),
        None => home_env_path(),
    };
    let env_source = expand_home_path(&env_source);
    let env_target = jekko_jnoccio_boot::unlock::jnoccio_env_path(&repo_root);
    let env_copied = copy_env_if_needed(&env_source, &env_target, args.force_env_copy)?;

    let developer_key_installed_users = if args.no_install_developer_key {
        Vec::new()
    } else {
        install_developer_key(&secret, args.user.as_deref())?
    };

    let body = json!({
        "status": "unlocked",
        "repo_root": unlock_report.repo_root,
        "plaintext_checkout": unlock_report.plaintext,
        "env_path": env_target,
        "env_copied": env_copied,
        "developer_key_installed_users": developer_key_installed_users,
    });
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

fn resolve_repo_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(expand_home_path(path));
    }
    if let Some(root) = jekko_jnoccio_boot::unlock::find_repo_root() {
        return Ok(root);
    }
    std::env::current_dir().context("resolve current directory")
}

fn default_secret_path() -> PathBuf {
    let base = match home_dir() {
        Some(home) => home,
        None => PathBuf::from("."),
    };
    base.join("jnoccio-fusion.unlock")
}

fn home_env_path() -> PathBuf {
    let base = match home_dir() {
        Some(home) => home,
        None => PathBuf::from("."),
    };
    base.join(".env.jnoccio")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn expand_home_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return match home_dir() {
            Some(home) => home,
            None => path.to_path_buf(),
        };
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

fn copy_env_if_needed(source: &Path, target: &Path, force: bool) -> Result<bool> {
    if !source.is_file() {
        return Ok(false);
    }
    if target.exists() && !force {
        return Ok(false);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::copy(source, target)
        .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
    set_secret_file_mode(target)?;
    Ok(true)
}

fn install_developer_key(secret: &str, user_id: Option<&str>) -> Result<Vec<String>> {
    let _ = upsert_env_key(&home_env_path(), "JNOCCIO_DEVELOPER_KEY", secret)?;
    let users_root = jekko_provider::key_pool::users_root()
        .context("resolve ~/.jekko/users for JNOCCIO_DEVELOPER_KEY install")?;
    let user_ids = match user_id {
        Some(user) => vec![user.to_string()],
        None => existing_llm_env_user_ids(&users_root)?,
    };

    let mut installed = Vec::with_capacity(user_ids.len());
    for user_id in user_ids {
        let user = jekko_provider::key_pool::user_dir(&users_root, &user_id);
        fs::create_dir_all(&user.dir).with_context(|| format!("create {}", user.dir.display()))?;
        let _ = upsert_env_key(&user.llm_env_path, "JNOCCIO_DEVELOPER_KEY", secret)?;
        installed.push(user.user_id);
    }
    Ok(installed)
}

fn existing_llm_env_user_ids(users_root: &Path) -> Result<Vec<String>> {
    let mut users = match fs::read_dir(users_root) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().ok().is_some_and(|item| item.is_dir()))
            .filter_map(|entry| {
                let user_id = entry.file_name().to_string_lossy().into_owned();
                let dir = jekko_provider::key_pool::user_dir(users_root, &user_id);
                dir.llm_env_path.is_file().then_some(user_id)
            })
            .collect::<Vec<_>>(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(err).with_context(|| format!("read {}", users_root.display())),
    };
    users.sort();
    if users.is_empty() {
        users.push(jekko_provider::key_pool::DEFAULT_USER_ID.to_string());
    }
    Ok(users)
}

fn user_ids_with_key(key: &str) -> Vec<String> {
    let Some(users_root) = jekko_provider::key_pool::users_root() else {
        return Vec::new();
    };
    let Ok(user_ids) = existing_llm_env_user_ids(&users_root) else {
        return Vec::new();
    };
    user_ids
        .into_iter()
        .filter(|user_id| {
            let user = jekko_provider::key_pool::user_dir(&users_root, user_id);
            env_file_has_key(&user.llm_env_path, key)
        })
        .collect()
}

fn upsert_env_key(path: &Path, key: &str, value: &str) -> Result<bool> {
    let mut text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err).with_context(|| format!("read {}", path.display())),
    };
    let mut changed = false;
    let mut found = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let without_export = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let name = without_export.split_once('=').map(|(name, _)| name.trim());
        if name == Some(key) {
            found = true;
            let replacement = format!("{key}={value}");
            if line != replacement {
                changed = true;
            }
            lines.push(replacement);
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        changed = true;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        lines.push(format!("{key}={value}"));
    }
    if changed {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let mut out = lines.join("\n");
        out.push('\n');
        fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
        set_secret_file_mode(path)?;
    }
    Ok(changed)
}

fn set_secret_file_mode(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod {}", path.display()))?;
    }
    Ok(())
}

fn env_file_has_key(path: &Path, key: &str) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        let without_export = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        without_export
            .split_once('=')
            .is_some_and(|(name, value)| name.trim() == key && !value.trim().is_empty())
    })
}
