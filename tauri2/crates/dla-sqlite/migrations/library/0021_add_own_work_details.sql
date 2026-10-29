CREATE TABLE library_own_work_details (
    installation_id TEXT PRIMARY KEY
        REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    circle TEXT,
    code TEXT,
    release_date TEXT,
    kind TEXT CHECK (
        kind IS NULL OR kind IN ('images', 'manga', 'game', 'voice', 'video', 'document')
    ),
    tags TEXT NOT NULL DEFAULT '[]',
    description TEXT,
    cover TEXT,
    samples TEXT NOT NULL DEFAULT '[]',
    updated_at TEXT NOT NULL CHECK (length(trim(updated_at)) > 0)
);
