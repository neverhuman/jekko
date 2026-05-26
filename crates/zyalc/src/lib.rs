//! zyalc — `.zyal` source compiler.
//!
//! Four profiles, disambiguated by top-of-file pragma:
//! - Profile A — runbook (sentinel-wrapped strict YAML, validation-only).
//! - Profile B — declarative (`# zyal: declarative target=toml ...`), emits TOML.
//! - Profile C — workflow (`# zyal: declarative target=github-workflow ...`), emits YAML.
//! - Profile D — SuperWorkflow (`# zyal: declarative target=superworkflow ...`), emits JSON.

pub mod compile;
pub mod live_audit;
pub mod profile;
pub mod replay_verify;
pub mod runbook_lint;
