//! Smoke test for `jekko --help`.
//!
//! Asserts every top-level subcommand label appears in the help text. Catches
//! a regression where a subcommand is wired into `cli.rs` but its module is
//! missing or returns an error before the help table is built.

use assert_cmd::Command;
use predicates::str::contains;

const EXPECTED_SUBCOMMANDS: &[&str] = &[
    "tui",
    "run",
    "serve",
    "session",
    "providers",
    "models",
    "keys",
    "agent",
    "mcp",
    "acp",
    "jankurai",
    "daemon",
    "plugin",
    "debug",
    "import",
    "export",
    "stats",
    "pr",
    "github",
    "db",
    "upgrade",
    "uninstall",
    "watch",
    "port-run",
];

fn help_output() -> String {
    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    let output = cmd.arg("--help").output().expect("help output");
    String::from_utf8(output.stdout).expect("help stdout is utf8")
}

#[test]
fn help_lists_every_subcommand() {
    let stdout = help_output();
    for name in EXPECTED_SUBCOMMANDS {
        assert!(
            stdout.contains(name),
            "expected subcommand `{name}` in help output:\n{stdout}"
        );
    }
}

#[test]
fn help_includes_at_least_fifteen_subcommands() {
    let stdout = help_output();
    let mut found = 0usize;
    for name in EXPECTED_SUBCOMMANDS {
        if stdout.contains(name) {
            found += 1;
        }
    }
    assert!(
        found >= 15,
        "expected at least 15 subcommands listed in help, found {found}"
    );
}

#[test]
fn help_mentions_examples_block() {
    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(contains("Examples"));
}

#[test]
fn short_h_alias_works() {
    let mut cmd = Command::cargo_bin("jekko").expect("jekko binary");
    cmd.arg("-h").assert().success();
}
