//! Sub-command implementations for `xtask`.
//!
//! Each module here implements one (or a small cluster of) `xtask`
//! subcommands. Keeping them in separate files prevents `src/main.rs`
//! from ballooning past the maintainable threshold (~1k LOC) once all
//! the Phase-1 commands land.

pub mod backend_contract;
pub mod ci_fast;
pub mod cli_help_parity;
pub mod db_migration_smoke;
pub mod git_hook;
pub mod httpapi_parity;
pub mod jankurai_gate;
pub mod openapi_check;
pub mod package;
pub mod parity_diff;
pub mod proof_receipt;
pub mod release;
pub mod schema;
pub mod security_lane;
pub mod session_fixture_parity;
pub mod split_common;
pub mod split_family;
pub mod split_ownership;
pub mod tool_schema_parity;
