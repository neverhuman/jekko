//! Built-in MCP server presets.
//!
//! Each preset is a small, vetted starting point for attaching a well-known
//! MCP server. The user runs:
//!
//! ```text
//! jekko mcp preset list                 # see what's available
//! jekko mcp preset add <name>           # write the stanza to mcp.toml
//! ```
//!
//! `add` writes a `[servers.<preset-name>]` entry pointing at the canonical
//! package source for that server (npm via `npx -y …` or PyPI via
//! `uvx …`), populated with the env-var names each server expects. The user
//! supplies the actual secret values via their own env at `jekko mcp status`
//! / `jekko run` time — secrets are never written to `mcp.toml` directly,
//! they're stored as `${VAR}` placeholders.
//!
//! If a preset's upstream package name has moved, the user can either
//! `jekko mcp detach <name>` and `jekko mcp attach <name> <new-command>`
//! manually, or hand-edit the resulting TOML. The preset library is a
//! convenience layer, not a hard contract.

use std::collections::BTreeMap;

use jekko_runtime::mcp::McpServerConfig;

/// One preset entry. All fields are `&'static` so the registry is a
/// compile-time constant.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    /// Preset name (also the default server name written into `mcp.toml`).
    pub name: &'static str,
    /// One-line description shown by `preset list`.
    pub description: &'static str,
    /// Command to spawn. Either `"npx"` or `"uvx"` for almost every
    /// preset; bare `"docker"` or a path is also allowed.
    pub command: &'static str,
    /// Arguments to pass after the command.
    pub args: &'static [&'static str],
    /// Env var names the spawned server expects. Stored as `${VAR}`
    /// placeholders in the persisted config so the user provides them at
    /// runtime; never written as resolved secrets.
    pub required_env: &'static [&'static str],
    /// Upstream homepage / install instructions for the user to consult.
    pub homepage: &'static str,
}

impl Preset {
    /// Build the `[servers.<name>]` config that gets written to `mcp.toml`.
    /// Env vars become `${VAR}` placeholders so no secrets leak to disk.
    pub fn to_server_config(&self) -> McpServerConfig {
        let mut env = BTreeMap::new();
        for var in self.required_env {
            env.insert((*var).to_string(), format!("${{{var}}}"));
        }
        McpServerConfig {
            transport: "stdio".to_string(),
            command: self.command.to_string(),
            args: self.args.iter().map(|s| s.to_string()).collect(),
            env,
            timeouts: BTreeMap::new(),
        }
    }
}

/// The full preset registry. Sorted alphabetically by name for stable
/// `preset list` output.
///
/// Package sources reflect the canonical entry point at time of writing;
/// callers who hit a moved package can override via `jekko mcp attach`.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "aws",
        description: "AWS read/write tooling (S3, IAM, EC2, Lambda) over stdio",
        command: "uvx",
        args: &["mcp-server-aws"],
        required_env: &["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_REGION"],
        homepage: "https://github.com/modelcontextprotocol/servers",
    },
    Preset {
        name: "claude",
        description: "Anthropic Claude API as an MCP server (model invocation)",
        command: "uvx",
        args: &["mcp-server-claude"],
        required_env: &["ANTHROPIC_API_KEY"],
        homepage: "https://docs.anthropic.com/en/docs/build-with-claude/mcp",
    },
    Preset {
        name: "gdrive",
        description: "Google Drive: list, read, search files",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-gdrive"],
        required_env: &["GOOGLE_APPLICATION_CREDENTIALS"],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/gdrive",
    },
    Preset {
        name: "github",
        description: "GitHub: repos, issues, PRs, code search",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-github"],
        required_env: &["GITHUB_PERSONAL_ACCESS_TOKEN"],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/github",
    },
    Preset {
        name: "huggingface",
        description: "Hugging Face Hub: models, datasets, spaces",
        command: "uvx",
        args: &["mcp-server-huggingface"],
        required_env: &["HF_TOKEN"],
        homepage: "https://huggingface.co/docs/hub/en/mcp",
    },
    Preset {
        name: "kubernetes",
        description: "Kubernetes cluster operations (kubectl-equivalent over MCP)",
        command: "npx",
        args: &["-y", "mcp-server-kubernetes"],
        required_env: &["KUBECONFIG"],
        homepage: "https://github.com/Flux159/mcp-server-kubernetes",
    },
    Preset {
        name: "linear",
        description: "Linear: issues, projects, cycles, comments",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-linear"],
        required_env: &["LINEAR_API_KEY"],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/linear",
    },
    Preset {
        name: "openai",
        description: "OpenAI API as an MCP server (model invocation)",
        command: "uvx",
        args: &["mcp-server-openai"],
        required_env: &["OPENAI_API_KEY"],
        homepage: "https://github.com/modelcontextprotocol/servers",
    },
    Preset {
        name: "vercel",
        description: "Vercel: deployments, projects, env vars",
        command: "npx",
        args: &["-y", "@vercel/mcp-adapter"],
        required_env: &["VERCEL_TOKEN"],
        homepage: "https://vercel.com/docs/integrations/mcp",
    },
];

/// Find a preset by case-insensitive name. Returns `None` if no match.
pub fn find_preset(name: &str) -> Option<&'static Preset> {
    let needle = name.to_ascii_lowercase();
    PRESETS.iter().find(|p| p.name == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_sorted_for_stable_list_output() {
        let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "PRESETS must be alphabetically sorted");
    }

    #[test]
    fn registry_has_no_duplicate_names() {
        let mut seen = std::collections::HashSet::new();
        for p in PRESETS {
            assert!(seen.insert(p.name), "duplicate preset name: {}", p.name);
        }
    }

    #[test]
    fn every_preset_has_canonical_stdio_command() {
        for p in PRESETS {
            assert!(
                matches!(p.command, "npx" | "uvx"),
                "preset `{}` uses non-canonical command `{}`; allowed: npx, uvx",
                p.name,
                p.command
            );
            assert!(
                !p.args.is_empty(),
                "preset `{}` has no args; must specify the package",
                p.name
            );
        }
    }

    #[test]
    fn npx_presets_use_dash_y_for_non_interactive_install() {
        for p in PRESETS {
            if p.command == "npx" {
                assert_eq!(
                    p.args[0], "-y",
                    "npx preset `{}` must use `-y` to skip the interactive install prompt",
                    p.name
                );
            }
        }
    }

    #[test]
    fn every_preset_declares_at_least_one_env_var() {
        for p in PRESETS {
            assert!(
                !p.required_env.is_empty(),
                "preset `{}` declares no required env vars; if truly none, document with a placeholder",
                p.name
            );
        }
    }

    #[test]
    fn env_var_names_are_screaming_snake() {
        let valid = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_';
        for p in PRESETS {
            for var in p.required_env {
                assert!(
                    var.chars().all(valid) && !var.is_empty(),
                    "preset `{}` has malformed env var `{}`; must be SCREAMING_SNAKE_CASE",
                    p.name,
                    var
                );
            }
        }
    }

    #[test]
    fn to_server_config_emits_placeholder_not_resolved_secret() {
        let github = find_preset("github").unwrap();
        let cfg = github.to_server_config();
        let placeholder = cfg.env.get("GITHUB_PERSONAL_ACCESS_TOKEN").unwrap();
        assert_eq!(placeholder, "${GITHUB_PERSONAL_ACCESS_TOKEN}");
        // Even if the env var IS set when to_server_config runs, the
        // PERSISTED value must remain the placeholder so no secret leaks
        // to disk.
        std::env::set_var("GITHUB_PERSONAL_ACCESS_TOKEN", "ghp_leak_test_value");
        let cfg2 = github.to_server_config();
        std::env::remove_var("GITHUB_PERSONAL_ACCESS_TOKEN");
        assert_eq!(
            cfg2.env.get("GITHUB_PERSONAL_ACCESS_TOKEN").unwrap(),
            "${GITHUB_PERSONAL_ACCESS_TOKEN}",
            "preset-derived config must not bake in resolved secrets"
        );
    }

    #[test]
    fn find_preset_is_case_insensitive() {
        assert!(find_preset("github").is_some());
        assert!(find_preset("GitHub").is_some());
        assert!(find_preset("GITHUB").is_some());
        assert!(find_preset("does-not-exist").is_none());
    }

    #[test]
    fn to_server_config_uses_stdio_transport() {
        for p in PRESETS {
            assert_eq!(p.to_server_config().transport, "stdio");
        }
    }

    #[test]
    fn nine_canonical_presets_present() {
        let expected = [
            "aws",
            "claude",
            "gdrive",
            "github",
            "huggingface",
            "kubernetes",
            "linear",
            "openai",
            "vercel",
        ];
        let actual: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        assert_eq!(actual, expected.to_vec());
    }
}
