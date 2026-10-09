ALTER TABLE library_scan_session
    ADD COLUMN purpose TEXT NOT NULL DEFAULT 'library'
    CHECK (purpose IN ('library', 'install'));

-- Install from DLA scanned its staging folders before scans had a purpose.
UPDATE library_scan_session
SET purpose = 'install'
WHERE root_id IN (
    SELECT root_id FROM library_scan_root
    WHERE instr(path_key, '/.dla-downloads/') > 0
);

CREATE INDEX library_scan_session_purpose_started
    ON library_scan_session(purpose, started_at DESC);

CREATE TABLE library_scan_ignored_finding (
    root_id TEXT NOT NULL REFERENCES library_scan_root(root_id) ON DELETE CASCADE,
    scope_key TEXT NOT NULL,
    ignored_at TEXT NOT NULL,
    PRIMARY KEY (root_id, scope_key)
);
