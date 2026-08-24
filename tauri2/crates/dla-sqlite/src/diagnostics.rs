use std::{fs, path::Path};

use dla_application::diagnostics::{ProbeCheck, ProbeReport, ProbeRunner};
use rusqlite::{Connection, params};
use rusqlite_migration::{M, Migrations};

use crate::database;

const PROBE_TEXT: &str = "星空の図書館 — RJ01648842 — 音声作品";
const PROBE_MIGRATION_LIST: &[M<'static>] = &[M::up(include_str!("../schema/diagnostics.sql"))];
const PROBE_MIGRATIONS: Migrations<'static> = Migrations::from_slice(PROBE_MIGRATION_LIST);

pub struct SqliteProbe;

impl SqliteProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SqliteProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl ProbeRunner for SqliteProbe {
    fn run(&self, database_path: &Path) -> ProbeReport {
        let mut report = ReportBuilder::new(database_path);
        let Some(parent) = database_path.parent() else {
            report.check(
                "storage_directory",
                "Private storage directory",
                false,
                "database path has no parent directory",
            );
            return report.finish();
        };
        if let Err(error) = fs::create_dir_all(parent) {
            report.check(
                "storage_directory",
                "Private storage directory",
                false,
                error.to_string(),
            );
            return report.finish();
        }
        report.check(
            "storage_directory",
            "Private storage directory",
            true,
            parent.display().to_string(),
        );

        let connection = match database::open(database_path, &PROBE_MIGRATIONS) {
            Ok(connection) => connection,
            Err(error) => {
                report.check(
                    "database_open",
                    "Open SQLite database",
                    false,
                    error.to_string(),
                );
                return report.finish();
            }
        };
        report.check(
            "database_open",
            "Open SQLite database",
            true,
            "database connection established",
        );

        match connection.query_row("SELECT sqlite_version()", [], |row| row.get(0)) {
            Ok(version) => {
                report.report.sqlite_version = version;
                report.check(
                    "sqlite_version",
                    "Read SQLite version",
                    true,
                    report.report.sqlite_version.clone(),
                );
            }
            Err(error) => {
                report.check(
                    "sqlite_version",
                    "Read SQLite version",
                    false,
                    error.to_string(),
                );
                return report.finish();
            }
        }
        report.check(
            "migrations",
            "Apply ordered migrations",
            true,
            "schema version 1 is current",
        );

        let (foreign_keys, foreign_key_detail) = check_foreign_keys(&connection);
        report.check(
            "foreign_keys",
            "Enforce foreign keys",
            foreign_keys,
            foreign_key_detail,
        );

        match connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0)) {
            Ok(mode) => {
                report.report.journal_mode = mode.clone();
                report.check(
                    "wal",
                    "Enable WAL journal mode",
                    mode.eq_ignore_ascii_case("wal"),
                    mode,
                );
            }
            Err(error) => report.check("wal", "Enable WAL journal mode", false, error.to_string()),
        }

        let (unicode, unicode_detail) = check_unicode_round_trip(&connection);
        report.check(
            "unicode_round_trip",
            "Round-trip Japanese text",
            unicode,
            unicode_detail,
        );
        drop(connection);
        match database::open(database_path, &PROBE_MIGRATIONS)
            .and_then(|reopened| read_probe_text(&reopened))
        {
            Ok(value) => report.check(
                "reopen",
                "Close and reopen database",
                value == PROBE_TEXT,
                value,
            ),
            Err(error) => report.check(
                "reopen",
                "Close and reopen database",
                false,
                error.to_string(),
            ),
        }

        report.finish()
    }
}

fn check_foreign_keys(connection: &Connection) -> (bool, String) {
    let enabled = match connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
    {
        Ok(enabled) => enabled,
        Err(error) => return (false, error.to_string()),
    };
    if let Err(error) = connection.execute("DELETE FROM diagnostic_probe_record WHERE id = 2", []) {
        return (false, error.to_string());
    }
    let accepted = connection.execute(
        "INSERT INTO diagnostic_probe_record (id, parent_id, value, updated_at) VALUES (2, 999, ?1, ?2)",
        params![PROBE_TEXT, database::now_rfc3339()],
    );
    if accepted.is_ok() {
        let _ = connection.execute("DELETE FROM diagnostic_probe_record WHERE id = 2", []);
        return (false, "invalid foreign key was accepted".to_owned());
    }
    if enabled != 1 {
        return (false, "PRAGMA foreign_keys is disabled".to_owned());
    }
    (true, "invalid foreign key rejected".to_owned())
}

fn check_unicode_round_trip(connection: &Connection) -> (bool, String) {
    if let Err(error) = connection.execute(
        "INSERT OR IGNORE INTO diagnostic_probe_parent (id) VALUES (1)",
        [],
    ) {
        return (false, error.to_string());
    }
    if let Err(error) = connection.execute(
        "INSERT INTO diagnostic_probe_record (id, parent_id, value, updated_at)
         VALUES (1, 1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![PROBE_TEXT, database::now_rfc3339()],
    ) {
        return (false, error.to_string());
    }
    match read_probe_text(connection) {
        Ok(value) => (value == PROBE_TEXT, value),
        Err(error) => (false, error.to_string()),
    }
}

fn read_probe_text(connection: &Connection) -> rusqlite::Result<String> {
    connection.query_row(
        "SELECT value FROM diagnostic_probe_record WHERE id = 1",
        [],
        |row| row.get(0),
    )
}

struct ReportBuilder {
    report: ProbeReport,
}

impl ReportBuilder {
    fn new(database_path: &Path) -> Self {
        Self {
            report: ProbeReport {
                passed: true,
                platform: format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
                database_path: database_path.display().to_string(),
                sqlite_version: String::new(),
                journal_mode: String::new(),
                completed_at: String::new(),
                checks: Vec::with_capacity(8),
            },
        }
    }

    fn check(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        passed: bool,
        detail: impl Into<String>,
    ) {
        self.report.checks.push(ProbeCheck {
            key: key.into(),
            label: label.into(),
            passed,
            detail: detail.into(),
        });
        self.report.passed &= passed;
    }

    fn finish(mut self) -> ProbeReport {
        self.report.completed_at = database::now_rfc3339();
        self.report
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn passes_the_native_sqlite_capability_gate() {
        let directory = tempdir().expect("temporary directory");
        let report = SqliteProbe::new().run(&directory.path().join("probe.sqlite"));
        assert!(report.passed, "{:#?}", report.checks);
        assert_eq!(report.journal_mode.to_lowercase(), "wal");
        assert_eq!(report.checks.len(), 8);
    }
}
