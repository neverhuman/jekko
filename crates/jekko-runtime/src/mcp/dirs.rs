//! Directory + file-path helpers for the MCP config.
//!
//! Honors `JEKKO_HOME` like the rest of the runtime (see
//! [`crate::key_pool::dirs::users_root`] — `JEKKO_HOME` is the canonical
//! per-install override). Falls back to `$HOME/.jekko/`. Default config
//! file is `mcp.toml` (dedicated to MCP — separate from `llm.env`,
//! sessions DB, etc).

use std::path::PathBuf;

/// Default filename for the MCP config under the Jekko home dir.
pub const MCP_CONFIG_FILENAME: &str = "mcp.toml";

/// Resolve the `$JEKKO_HOME` or `$HOME/.jekko` root.
///
/// Returns `None` when neither `JEKKO_HOME` nor `HOME` is set — callers
/// should treat that as "no candidates" and refuse to operate.
pub fn jekko_home() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("JEKKO_HOME") {
        let path = PathBuf::from(custom);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".jekko"))
}

/// Resolve the default `$JEKKO_HOME/mcp.toml`. Returns `None` when no home
/// dir is available.
pub fn default_mcp_config_path() -> Option<PathBuf> {
    jekko_home().map(|h| h.join(MCP_CONFIG_FILENAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env-var manipulation tests must run serially: the global env is shared.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], body: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        body();
        for (k, v) in saved {
            match v {
                Some(val) => std::env::set_var(&k, &val),
                None => std::env::remove_var(&k),
            }
        }
    }

    #[test]
    fn jekko_home_honors_jekko_home_env() {
        with_env(
            &[
                ("JEKKO_HOME", Some("/tmp/custom_jekko_home")),
                ("HOME", Some("/should/not/use")),
            ],
            || {
                assert_eq!(
                    jekko_home().unwrap(),
                    PathBuf::from("/tmp/custom_jekko_home")
                );
            },
        );
    }

    #[test]
    fn jekko_home_falls_back_to_home_dot_jekko() {
        with_env(
            &[("JEKKO_HOME", None), ("HOME", Some("/tmp/myhome"))],
            || {
                assert_eq!(jekko_home().unwrap(), PathBuf::from("/tmp/myhome/.jekko"));
            },
        );
    }

    #[test]
    fn jekko_home_returns_none_when_neither_set() {
        with_env(&[("JEKKO_HOME", None), ("HOME", None)], || {
            assert!(jekko_home().is_none());
        });
    }

    #[test]
    fn jekko_home_ignores_empty_jekko_home() {
        with_env(
            &[("JEKKO_HOME", Some("")), ("HOME", Some("/tmp/h"))],
            || {
                assert_eq!(jekko_home().unwrap(), PathBuf::from("/tmp/h/.jekko"));
            },
        );
    }

    #[test]
    fn default_mcp_config_path_appends_filename() {
        with_env(
            &[("JEKKO_HOME", Some("/tmp/j")), ("HOME", Some("/x"))],
            || {
                assert_eq!(
                    default_mcp_config_path().unwrap(),
                    PathBuf::from("/tmp/j/mcp.toml")
                );
            },
        );
    }
}
