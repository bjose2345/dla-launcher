use std::{fs, path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags};
use rusqlite_migration::Migrations;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub fn open(path: &Path, migrations: &Migrations<'_>) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
    }

    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    migrations
        .to_latest(&mut connection)
        .map_err(migration_error)?;
    enable_wal(&connection)?;
    Ok(connection)
}

pub fn enable_wal(connection: &Connection) -> rusqlite::Result<String> {
    let journal_mode = connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(journal_mode)
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC 3339 timestamp")
}

fn migration_error(error: rusqlite_migration::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(error.into())
}
