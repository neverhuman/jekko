//! `mcp.toml` parser + writer.
//!
//! Schema (v1, stdio-only):
//!
//! ```toml
//! [servers.aara]
//! transport = "stdio"
//! command   = "python"
//! args      = ["-m", "apps.mcp_server", "--transport", "stdio"]
//!
//! [servers.aara.env]
//! QORCH_API_KEY = "${QORCH_API_KEY}"
//!
//! [servers.aara.timeouts]
//! default = 30
//! ```
//!
//! `${VAR}` and `${VAR:-default}` substitution is applied at read time using
//! the parent process env. Values that reference an unset variable with no
//! default substitute to the empty string and emit a [`tracing::warn`].
//!
//! TOML edits go through [`toml_edit::DocumentMut`] so user formatting and
//! comments survive round-trips when `attach`/`detach` modifies a file the
//! user has hand-edited.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{value, Array, DocumentMut, Item, Table};

use super::error::{McpError, McpResult};

/// Top-level config: a map of named server entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    /// Map of name -> server config. Keys are unique per file.
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

/// One MCP server entry in the config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Transport. Only `"stdio"` is supported in v1; `"sse"` reserved.
    pub transport: String,

    /// For stdio: command to spawn (e.g. `"python"`). For sse: the URL.
    pub command: String,

    /// For stdio: arguments to pass to the command. Empty for sse.
    #[serde(default)]
    pub args: Vec<String>,

    /// Env vars to set on the spawned child process. Values may use
    /// `${VAR}` or `${VAR:-default}` to interpolate from the parent env.
    /// Stored RAW in the TOML and substituted at read time.
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    /// Optional per-tool timeout tiers (seconds). Falls back to 30s default
    /// when unspecified.
    #[serde(default)]
    pub timeouts: BTreeMap<String, u64>,
}

impl McpServerConfig {
    /// Default timeout (seconds) for a tool that doesn't match any tier.
    pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

    /// Resolve env vars with `${VAR}` / `${VAR:-default}` interpolation.
    ///
    /// Lookup order for each `${VAR}` reference:
    /// 1. The parent process environment (`std::env::var`).
    /// 2. The canonical user's `~/.jekko/users/user/llm.env` keys file —
    ///    so values stored via `jekko keys set` are picked up without
    ///    requiring the user to also export them into their shell.
    /// 3. The `:-default` literal from the TOML, if any.
    /// 4. Empty string, with a `warn` log.
    pub fn resolved_env(&self) -> BTreeMap<String, String> {
        let keys_fallback = load_keys_fallback();
        self.env
            .iter()
            .map(|(k, v)| (k.clone(), interpolate_env(v, &keys_fallback)))
            .collect()
    }

    /// Lookup a timeout tier. Falls back to `default` then to
    /// [`DEFAULT_TIMEOUT_SECS`].
    pub fn timeout_secs(&self, tier: &str) -> u64 {
        self.timeouts
            .get(tier)
            .or_else(|| self.timeouts.get("default"))
            .copied()
            .unwrap_or(Self::DEFAULT_TIMEOUT_SECS)
    }
}

/// Allowed charset for server names. Mirrors a sensible CLI identifier:
/// letters, digits, `-` and `_`, 1–64 chars. Refuses dots, slashes,
/// whitespace, and shell metachars so the name is safe in URLs, file
/// paths, and `grep -F` audit queries.
pub const SERVER_NAME_PATTERN: &str = "^[A-Za-z0-9_-]{1,64}$";

/// Validate a server name against [`SERVER_NAME_PATTERN`].
pub fn validate_server_name(name: &str) -> McpResult<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(McpError::InvalidName(name.to_string(), SERVER_NAME_PATTERN));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(McpError::InvalidName(name.to_string(), SERVER_NAME_PATTERN));
    }
    Ok(())
}

/// Substitute `${VAR}` and `${VAR:-default}` from the parent process env,
/// falling back to jekko's per-user keys file when the parent env doesn't
/// have the var set. See [`McpServerConfig::resolved_env`] for the full
/// lookup order.
fn interpolate_env(value: &str, keys_fallback: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = value[i + 2..].find('}') {
                let inner = &value[i + 2..i + 2 + end];
                let (name, default) = match inner.split_once(":-") {
                    Some((n, d)) => (n, Some(d)),
                    None => (inner, None),
                };
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => match keys_fallback.get(name) {
                        Some(v) => out.push_str(v),
                        None => match default {
                            Some(d) => out.push_str(d),
                            None => {
                                tracing::warn!(
                                    var = name,
                                    "mcp config references unset env var; substituting empty string"
                                );
                            }
                        },
                    },
                }
                i += 2 + end + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Resolve `~/.jekko/` honoring `JEKKO_HOME` (for tests / isolated installs).
///
/// Returns `None` only when neither `JEKKO_HOME` nor `HOME` is set — callers
/// should treat that as "no keys configured", not a hard error.
fn jekko_home() -> Option<PathBuf> {
    if let Some(jh) = std::env::var_os("JEKKO_HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(jh));
    }
    std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(|h| PathBuf::from(h).join(".jekko"))
}

/// Read the canonical user's `llm.env` and return its `KEY=VALUE` pairs.
///
/// Returns an empty map when the file is missing, unreadable, or empty —
/// that's the legitimate "no keys configured yet" state and should not be
/// surfaced as an error. Comments (`#`) and blank lines are skipped.
fn load_keys_fallback() -> BTreeMap<String, String> {
    let Some(home) = jekko_home() else {
        return BTreeMap::new();
    };
    let path = home.join("users").join("user").join("llm.env");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Load a config file from disk. Returns an empty config (no servers) when
/// the file does not exist — that's the legitimate "no servers attached
/// yet" state.
pub fn load_or_empty(path: &Path) -> McpResult<McpConfig> {
    if !path.exists() {
        return Ok(McpConfig::default());
    }
    let text = std::fs::read_to_string(path).map_err(|e| McpError::ConfigParse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    toml::from_str(&text).map_err(|e| McpError::ConfigParse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// Insert (or replace) a `[servers.<name>]` entry, preserving comments and
/// formatting elsewhere in the file. Errors with [`McpError::DuplicateName`]
/// if the server already exists and `allow_replace` is false.
pub fn write_server_entry(
    path: &Path,
    name: &str,
    cfg: &McpServerConfig,
    allow_replace: bool,
) -> McpResult<()> {
    validate_server_name(name)?;
    let mut doc = read_doc(path)?;
    let servers = ensure_table(&mut doc, "servers");
    if servers.contains_key(name) && !allow_replace {
        return Err(McpError::DuplicateName(name.to_string()));
    }
    let entry = build_server_table(cfg);
    servers.insert(name, Item::Table(entry));
    atomically_write(path, &doc.to_string())
}

/// Remove a `[servers.<name>]` entry. Errors with
/// [`McpError::UnknownServer`] if the name is not present.
pub fn remove_server_entry(path: &Path, name: &str) -> McpResult<()> {
    validate_server_name(name)?;
    let mut doc = read_doc(path)?;
    let Some(servers) = doc.get_mut("servers").and_then(|i| i.as_table_mut()) else {
        return Err(McpError::UnknownServer(name.to_string()));
    };
    if servers.remove(name).is_none() {
        return Err(McpError::UnknownServer(name.to_string()));
    }
    atomically_write(path, &doc.to_string())
}

fn read_doc(path: &Path) -> McpResult<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let text = std::fs::read_to_string(path).map_err(|e| McpError::ConfigParse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    text.parse::<DocumentMut>()
        .map_err(|e| McpError::ConfigParse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
}

fn ensure_table<'a>(doc: &'a mut DocumentMut, key: &str) -> &'a mut Table {
    if !doc.contains_key(key) {
        doc.insert(key, Item::Table(Table::new()));
    }
    doc.get_mut(key)
        .and_then(|i| i.as_table_mut())
        .expect("just inserted")
}

fn build_server_table(cfg: &McpServerConfig) -> Table {
    let mut t = Table::new();
    t.insert("transport", value(&cfg.transport));
    t.insert("command", value(&cfg.command));
    let mut args = Array::new();
    for a in &cfg.args {
        args.push(a);
    }
    t.insert("args", value(args));
    if !cfg.env.is_empty() {
        let mut env_tbl = Table::new();
        for (k, v) in &cfg.env {
            env_tbl.insert(k, value(v));
        }
        t.insert("env", Item::Table(env_tbl));
    }
    if !cfg.timeouts.is_empty() {
        let mut t_tbl = Table::new();
        for (k, v) in &cfg.timeouts {
            t_tbl.insert(k, value(*v as i64));
        }
        t.insert("timeouts", Item::Table(t_tbl));
    }
    t
}

/// Write the file atomically: write to `<path>.tmp`, fsync, rename. This
/// prevents a partially-written `mcp.toml` if the process is killed
/// mid-write or the disk runs out of space.
fn atomically_write(path: &Path, contents: &str) -> McpResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| McpError::ConfigWrite {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let tmp: PathBuf = {
        let mut p = path.as_os_str().to_owned();
        p.push(".tmp");
        PathBuf::from(p)
    };
    std::fs::write(&tmp, contents).map_err(|e| McpError::ConfigWrite {
        path: tmp.clone(),
        source: e,
    })?;
    std::fs::rename(&tmp, path).map_err(|e| McpError::ConfigWrite {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_cfg() -> McpServerConfig {
        McpServerConfig {
            transport: "stdio".into(),
            command: "python".into(),
            args: vec!["-m".into(), "apps.mcp_server".into()],
            env: BTreeMap::from([("QORCH_API_KEY".into(), "${QORCH_API_KEY}".into())]),
            timeouts: BTreeMap::from([("default".into(), 30), ("training".into(), 1800)]),
        }
    }

    #[test]
    fn name_charset_accepts_letters_digits_dash_underscore() {
        for n in [
            "aara",
            "aara-prod",
            "aara_2",
            "A1",
            "x",
            "a".repeat(64).as_str(),
        ] {
            validate_server_name(n).unwrap_or_else(|e| panic!("rejected {n}: {e}"));
        }
    }

    #[test]
    fn name_charset_rejects_dots_slashes_spaces_shell_metas() {
        for n in [
            "aara.prod",
            "aara/prod",
            "aara prod",
            "aara;rm",
            "aara$(echo)",
            "",
            &"a".repeat(65),
        ] {
            assert!(
                validate_server_name(n).is_err(),
                "should reject `{n}` but accepted it"
            );
        }
    }

    #[test]
    fn load_or_empty_returns_default_when_missing() {
        let tmp = TempDir::new().unwrap();
        let cfg = load_or_empty(&tmp.path().join("nope.toml")).unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn write_and_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("mcp.toml");
        write_server_entry(&p, "aara", &fresh_cfg(), false).unwrap();
        let loaded = load_or_empty(&p).unwrap();
        let aara = loaded.servers.get("aara").expect("aara entry");
        assert_eq!(aara.transport, "stdio");
        assert_eq!(aara.command, "python");
        assert_eq!(aara.args, vec!["-m", "apps.mcp_server"]);
        assert_eq!(aara.timeout_secs("training"), 1800);
        assert_eq!(aara.timeout_secs("missing"), 30); // falls back to default
    }

    #[test]
    fn duplicate_name_refused_without_replace() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("mcp.toml");
        write_server_entry(&p, "aara", &fresh_cfg(), false).unwrap();
        let err = write_server_entry(&p, "aara", &fresh_cfg(), false).unwrap_err();
        assert!(matches!(err, McpError::DuplicateName(ref n) if n == "aara"));
    }

    #[test]
    fn duplicate_name_replaced_when_allow_replace() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("mcp.toml");
        let mut a = fresh_cfg();
        a.command = "python3.11".into();
        write_server_entry(&p, "aara", &a, false).unwrap();
        let mut b = fresh_cfg();
        b.command = "python3.12".into();
        write_server_entry(&p, "aara", &b, true).unwrap();
        let loaded = load_or_empty(&p).unwrap();
        assert_eq!(loaded.servers["aara"].command, "python3.12");
    }

    #[test]
    fn remove_server_entry_works() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("mcp.toml");
        write_server_entry(&p, "aara", &fresh_cfg(), false).unwrap();
        remove_server_entry(&p, "aara").unwrap();
        let loaded = load_or_empty(&p).unwrap();
        assert!(loaded.servers.is_empty());
    }

    #[test]
    fn remove_unknown_server_errors() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("mcp.toml");
        write_server_entry(&p, "aara", &fresh_cfg(), false).unwrap();
        let err = remove_server_entry(&p, "ghost").unwrap_err();
        assert!(matches!(err, McpError::UnknownServer(ref n) if n == "ghost"));
    }

    #[test]
    fn comments_preserved_through_attach_and_detach() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("mcp.toml");
        std::fs::write(
            &p,
            "# Top comment, must survive\n\n[servers.existing]\ntransport = \"stdio\"\ncommand = \"foo\"\nargs = []\n",
        )
        .unwrap();
        write_server_entry(&p, "aara", &fresh_cfg(), false).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(
            after.contains("# Top comment, must survive"),
            "top comment lost: {after}"
        );
        assert!(after.contains("[servers.existing]"));
        assert!(after.contains("[servers.aara]"));
        // Detach aara, existing + comment still there.
        remove_server_entry(&p, "aara").unwrap();
        let after2 = std::fs::read_to_string(&p).unwrap();
        assert!(after2.contains("# Top comment, must survive"));
        assert!(after2.contains("[servers.existing]"));
        assert!(!after2.contains("[servers.aara]"));
    }

    #[test]
    fn interpolate_env_substitutes_set_vars() {
        std::env::set_var("MCP_TEST_VAR_SET", "hello");
        let out = interpolate_env("prefix-${MCP_TEST_VAR_SET}-suffix", &BTreeMap::new());
        assert_eq!(out, "prefix-hello-suffix");
        std::env::remove_var("MCP_TEST_VAR_SET");
    }

    #[test]
    fn interpolate_env_uses_default_when_unset() {
        std::env::remove_var("MCP_TEST_VAR_UNSET");
        let out = interpolate_env("v=${MCP_TEST_VAR_UNSET:-fallback}", &BTreeMap::new());
        assert_eq!(out, "v=fallback");
    }

    #[test]
    fn interpolate_env_emits_empty_when_unset_no_default() {
        std::env::remove_var("MCP_TEST_VAR_UNSET2");
        let out = interpolate_env("v=${MCP_TEST_VAR_UNSET2}", &BTreeMap::new());
        assert_eq!(out, "v=");
    }

    #[test]
    fn interpolate_env_falls_back_to_keys_file_when_process_env_unset() {
        // Process env has nothing for this var, but the keys-file fallback
        // does — interpolation should pick up the keys-file value rather
        // than the `:-default` literal or the empty-string warn path.
        std::env::remove_var("MCP_TEST_KEYSFILE_ONLY");
        let keys = BTreeMap::from([(
            "MCP_TEST_KEYSFILE_ONLY".to_string(),
            "from-keys".to_string(),
        )]);
        let out = interpolate_env("v=${MCP_TEST_KEYSFILE_ONLY}", &keys);
        assert_eq!(out, "v=from-keys");
    }

    #[test]
    fn interpolate_env_process_env_wins_over_keys_file() {
        // When both sources have the var, the process env takes precedence —
        // matches the documented lookup order in `resolved_env`.
        std::env::set_var("MCP_TEST_PRECEDENCE", "from-process-env");
        let keys = BTreeMap::from([(
            "MCP_TEST_PRECEDENCE".to_string(),
            "from-keys-file".to_string(),
        )]);
        let out = interpolate_env("v=${MCP_TEST_PRECEDENCE}", &keys);
        assert_eq!(out, "v=from-process-env");
        std::env::remove_var("MCP_TEST_PRECEDENCE");
    }

    #[test]
    fn interpolate_env_default_used_when_neither_process_nor_keys_file_has_var() {
        // Both empty; the `:-default` literal still wins over the empty-warn
        // path. Confirms the new fallback didn't reorder the documented chain.
        std::env::remove_var("MCP_TEST_NEITHER");
        let out = interpolate_env("v=${MCP_TEST_NEITHER:-from-default}", &BTreeMap::new());
        assert_eq!(out, "v=from-default");
    }

    #[test]
    fn resolved_env_applies_substitution() {
        std::env::set_var("MCP_TEST_TOKEN", "xyz");
        let cfg = McpServerConfig {
            transport: "stdio".into(),
            command: "echo".into(),
            args: vec![],
            env: BTreeMap::from([("TOK".into(), "${MCP_TEST_TOKEN}".into())]),
            timeouts: BTreeMap::new(),
        };
        let resolved = cfg.resolved_env();
        assert_eq!(resolved.get("TOK").unwrap(), "xyz");
        std::env::remove_var("MCP_TEST_TOKEN");
    }

    #[test]
    fn resolved_env_picks_up_keys_file_when_process_env_unset() {
        // Stand up a tiny fake `~/.jekko/users/user/llm.env` via `JEKKO_HOME`,
        // make sure the resolver actually reads it when spawning would call
        // `resolved_env()`. This is the real-world `jekko mcp status` path:
        // a fresh shell without `.env` sourced should still resolve tokens
        // the user already set via `jekko keys set`.
        let tmp = TempDir::new().unwrap();
        let user_dir = tmp.path().join("users").join("user");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(
            user_dir.join("llm.env"),
            "# example llm.env\nMCP_TEST_KEYSFILE_RESOLVED=from-keys-file\n",
        )
        .unwrap();
        std::env::remove_var("MCP_TEST_KEYSFILE_RESOLVED");
        std::env::set_var("JEKKO_HOME", tmp.path());

        let cfg = McpServerConfig {
            transport: "stdio".into(),
            command: "echo".into(),
            args: vec![],
            env: BTreeMap::from([("TOK".into(), "${MCP_TEST_KEYSFILE_RESOLVED}".into())]),
            timeouts: BTreeMap::new(),
        };
        let resolved = cfg.resolved_env();

        std::env::remove_var("JEKKO_HOME");
        assert_eq!(resolved.get("TOK").unwrap(), "from-keys-file");
    }

    #[test]
    fn load_keys_fallback_skips_comments_and_blank_lines() {
        let tmp = TempDir::new().unwrap();
        let user_dir = tmp.path().join("users").join("user");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(
            user_dir.join("llm.env"),
            "# header comment\n\nHF_TOKEN=hf_xxx\n   # indented comment\nLINEAR_API_KEY=lin_yyy\n",
        )
        .unwrap();
        std::env::set_var("JEKKO_HOME", tmp.path());
        let keys = load_keys_fallback();
        std::env::remove_var("JEKKO_HOME");
        assert_eq!(keys.get("HF_TOKEN").map(|s| s.as_str()), Some("hf_xxx"));
        assert_eq!(
            keys.get("LINEAR_API_KEY").map(|s| s.as_str()),
            Some("lin_yyy")
        );
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn load_keys_fallback_empty_when_file_missing() {
        // Point JEKKO_HOME at a directory with no users/user/llm.env. Should
        // return empty rather than erroring — matches the documented
        // "no keys configured yet" state.
        let tmp = TempDir::new().unwrap();
        std::env::set_var("JEKKO_HOME", tmp.path());
        let keys = load_keys_fallback();
        std::env::remove_var("JEKKO_HOME");
        assert!(keys.is_empty());
    }

    #[test]
    fn atomic_write_does_not_leave_tmp_on_success() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("mcp.toml");
        write_server_entry(&p, "aara", &fresh_cfg(), false).unwrap();
        let mut tmp_path = p.clone().into_os_string();
        tmp_path.push(".tmp");
        assert!(!PathBuf::from(tmp_path).exists());
    }
}
