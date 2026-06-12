use std::path::PathBuf;

use serde::Serialize;

pub enum CompileOutcome {
    Wrote(PathBuf),
    Unchanged(PathBuf),
    Drift(PathBuf),
}

pub use CompileOutcome as Outcome;

#[derive(Debug, Serialize)]
pub struct CompileReport {
    pub compiled: Vec<PathBuf>,
    pub unchanged: Vec<PathBuf>,
    pub drifted: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct InspectInfo {
    pub profile: String,
    pub target: Option<PathBuf>,
    pub schema: Option<String>,
}
