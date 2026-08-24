use std::{path::Path, sync::Arc};

use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeCheck {
    pub key: String,
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub passed: bool,
    pub platform: String,
    pub database_path: String,
    pub sqlite_version: String,
    pub journal_mode: String,
    pub completed_at: String,
    pub checks: Vec<ProbeCheck>,
}

pub trait ProbeRunner: Send + Sync {
    fn run(&self, database_path: &Path) -> ProbeReport;
}

pub struct DiagnosticsService {
    database_path: std::path::PathBuf,
    runner: Arc<dyn ProbeRunner>,
}

impl DiagnosticsService {
    pub fn new(database_path: std::path::PathBuf, runner: Arc<dyn ProbeRunner>) -> Self {
        Self {
            database_path,
            runner,
        }
    }

    pub fn run_sqlite_probe(&self) -> ProbeReport {
        self.runner.run(&self.database_path)
    }
}
