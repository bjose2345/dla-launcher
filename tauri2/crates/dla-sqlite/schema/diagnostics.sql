-- Current empty database schema for the first public DLA Launcher release.
-- Future releases add ordered migrations after this baseline.

CREATE TABLE diagnostic_probe_parent (
    id INTEGER PRIMARY KEY
) STRICT;

CREATE TABLE diagnostic_probe_record (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER NOT NULL REFERENCES diagnostic_probe_parent(id),
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
