use super::env_snapshot::{env_snapshot, supported_runtime_snapshot};
use super::*;
use std::fs;
use std::sync::Mutex;
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    prev_home: Option<std::ffi::OsString>,
    prev_jekko_home: Option<std::ffi::OsString>,
    prev_dev: Option<std::ffi::OsString>,
    prev_policy: Option<std::ffi::OsString>,
    prev_openai: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn install(home: &std::path::Path, dev_key: Option<&str>) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_home = std::env::var_os("HOME");
        let prev_jekko_home = std::env::var_os("JEKKO_HOME");
        let prev_dev = std::env::var_os("JNOCCIO_DEVELOPER_KEY");
        let prev_policy = std::env::var_os("JEKKO_KEY_SOURCE_POLICY");
        let prev_openai = std::env::var_os("OPENAI_API_KEY");
        std::env::set_var("HOME", home);
        std::env::remove_var("JEKKO_HOME");
        std::env::remove_var("JEKKO_KEY_SOURCE_POLICY");
        std::env::remove_var("OPENAI_API_KEY");
        match dev_key {
            Some(v) => std::env::set_var("JNOCCIO_DEVELOPER_KEY", v),
            None => std::env::remove_var("JNOCCIO_DEVELOPER_KEY"),
        }
        reset_balancer_for_tests();
        Self {
            prev_home,
            prev_jekko_home,
            prev_dev,
            prev_policy,
            prev_openai,
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
        match &self.prev_jekko_home {
            Some(v) => std::env::set_var("JEKKO_HOME", v),
            None => std::env::remove_var("JEKKO_HOME"),
        }
        match &self.prev_dev {
            Some(v) => std::env::set_var("JNOCCIO_DEVELOPER_KEY", v),
            None => std::env::remove_var("JNOCCIO_DEVELOPER_KEY"),
        }
        match &self.prev_policy {
            Some(v) => std::env::set_var("JEKKO_KEY_SOURCE_POLICY", v),
            None => std::env::remove_var("JEKKO_KEY_SOURCE_POLICY"),
        }
        match &self.prev_openai {
            Some(v) => std::env::set_var("OPENAI_API_KEY", v),
            None => std::env::remove_var("OPENAI_API_KEY"),
        }
        reset_balancer_for_tests();
    }
}

#[test]
fn provider_selection_skips_jnoccio_without_developer_unlock() {
    let home = TempDir::new().unwrap();
    let _guard = EnvGuard::install(home.path(), None);

    let snapshot = env_snapshot();
    let selection = choose_active_provider(&snapshot, jekko_jnoccio_boot::unlock::is_unlocked());
    assert_ne!(selection.active_provider_id.as_deref(), Some("jnoccio"));
}

#[test]
fn provider_selection_accepts_developer_key_from_home_env_file() {
    let home = TempDir::new().unwrap();
    fs::write(
        home.path().join(".env.jnoccio"),
        "JNOCCIO_DEVELOPER_KEY=file-secret\n",
    )
    .unwrap();
    let _guard = EnvGuard::install(home.path(), None);

    let snapshot = env_snapshot();
    let selection = choose_active_provider(&snapshot, jekko_jnoccio_boot::unlock::is_unlocked());
    assert_eq!(selection.active_provider_id.as_deref(), Some("jnoccio"));
}

#[test]
fn provider_selection_sees_default_user_key_pool() {
    let home = TempDir::new().unwrap();
    let user_dir = home.path().join(".jekko/users/user");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(user_dir.join("llm.env"), "OPENROUTER_API_KEY=key\n").unwrap();
    let _guard = EnvGuard::install(home.path(), None);

    let snapshot = env_snapshot();
    let selection = choose_active_provider(&snapshot, jekko_jnoccio_boot::unlock::is_unlocked());
    assert_eq!(selection.active_provider_id.as_deref(), Some("openrouter"));
}

#[test]
fn provider_selection_sees_extra_user_key_pool_without_unlock() {
    let home = TempDir::new().unwrap();
    let user_dir = home.path().join(".jekko/users/user_1");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(user_dir.join("llm.env"), "OPENROUTER_API_KEY=key\n").unwrap();
    let _guard = EnvGuard::install(home.path(), None);

    let snapshot = env_snapshot();
    let selection = choose_active_provider(
        &supported_runtime_snapshot(&snapshot),
        jekko_jnoccio_boot::unlock::is_unlocked(),
    );
    assert_eq!(selection.active_provider_id.as_deref(), Some("openrouter"));
}

#[test]
fn provider_selection_skips_configured_but_unsupported_provider() {
    let home = TempDir::new().unwrap();
    let user_dir = home.path().join(".jekko/users/user_1");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(
        user_dir.join("llm.env"),
        "GEMINI_API_KEY=google-key\nOPENROUTER_API_KEY=openrouter-key\n",
    )
    .unwrap();
    let _guard = EnvGuard::install(home.path(), None);

    let snapshot = env_snapshot();
    let selection = choose_active_provider(
        &supported_runtime_snapshot(&snapshot),
        jekko_jnoccio_boot::unlock::is_unlocked(),
    );
    assert_eq!(selection.active_provider_id.as_deref(), Some("openrouter"));
}

#[test]
fn jnoccio_credential_uses_local_default_without_developer_key() {
    let home = TempDir::new().unwrap();
    let _guard = EnvGuard::install(home.path(), None);

    let selected = select_credential("jnoccio", "jnoccio/jnoccio-fusion")
        .unwrap()
        .expect("jnoccio local credential");
    assert!(selected.user_id.is_none());
    assert!(matches!(
        selected.credential,
        ProviderCredential::ApiKey { ref key } if key == JNOCCIO_DEFAULT_API_KEY
    ));
}

#[test]
fn users_only_ignores_process_env_key() {
    let home = TempDir::new().unwrap();
    let _guard = EnvGuard::install(home.path(), None);
    std::env::set_var("OPENAI_API_KEY", "process-key");
    std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");

    let snapshot = env_snapshot();
    let selection = choose_active_provider(&supported_runtime_snapshot(&snapshot), true);
    assert_eq!(selection.active_provider_id, None);
    assert!(select_credential("openai", "gpt-5").unwrap().is_none());
}

#[test]
fn users_only_ignores_home_env_jnoccio_file() {
    let home = TempDir::new().unwrap();
    fs::write(
        home.path().join(".env.jnoccio"),
        "JNOCCIO_DEVELOPER_KEY=file-secret\n",
    )
    .unwrap();
    let _guard = EnvGuard::install(home.path(), None);
    std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");

    let snapshot = env_snapshot();
    let selection = choose_active_provider(&supported_runtime_snapshot(&snapshot), true);
    assert_ne!(selection.active_provider_id.as_deref(), Some("jnoccio"));
}

#[test]
fn users_only_rejects_jnoccio_local_default() {
    let home = TempDir::new().unwrap();
    let user_dir = home.path().join(".jekko/users/user");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(
        user_dir.join("llm.env"),
        format!("JNOCCIO_DEVELOPER_KEY={JNOCCIO_DEFAULT_API_KEY}\n"),
    )
    .unwrap();
    let _guard = EnvGuard::install(home.path(), None);
    std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");
    reset_balancer_for_tests();

    assert!(select_credential("jnoccio", "jnoccio-fusion")
        .unwrap()
        .is_none());
}

#[test]
fn users_only_allows_jnoccio_when_key_is_in_user_llm_env() {
    let home = TempDir::new().unwrap();
    let user_dir = home.path().join(".jekko/users/user_1");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(
        user_dir.join("llm.env"),
        "JNOCCIO_DEVELOPER_KEY=user-secret\n",
    )
    .unwrap();
    let _guard = EnvGuard::install(home.path(), None);
    std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");
    reset_balancer_for_tests();

    let snapshot = env_snapshot();
    let selection = choose_active_provider(&supported_runtime_snapshot(&snapshot), true);
    assert_eq!(selection.active_provider_id.as_deref(), Some("jnoccio"));
    let selected = select_credential("jnoccio", "jnoccio-fusion")
        .unwrap()
        .expect("user llm.env credential");
    assert_eq!(selected.user_id.as_deref(), Some("user_1"));
}

#[test]
fn selected_credential_in_users_only_always_has_user_id() {
    let home = TempDir::new().unwrap();
    let user_dir = home.path().join(".jekko/users/user");
    fs::create_dir_all(&user_dir).unwrap();
    fs::write(user_dir.join("llm.env"), "OPENAI_API_KEY=user-key\n").unwrap();
    let _guard = EnvGuard::install(home.path(), None);
    std::env::set_var("JEKKO_KEY_SOURCE_POLICY", "users-only");
    reset_balancer_for_tests();

    let selected = select_credential("openai", "gpt-5")
        .unwrap()
        .expect("user llm.env credential");
    assert_eq!(selected.user_id.as_deref(), Some("user"));
}
