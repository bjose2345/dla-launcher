-- Games translated with the launcher's plugin, and every file the launcher
-- added to or changed in their folders, so removal can put them back.
CREATE TABLE library_game_translation (
    installation_id TEXT PRIMARY KEY
        REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    engine TEXT NOT NULL,
    game_folder TEXT,
    shown INTEGER NOT NULL,
    plugin TEXT NOT NULL,
    turned_on_at TEXT NOT NULL,
    paused INTEGER NOT NULL DEFAULT 0,
    -- The last time the whole game was translated ahead of play.
    prepared_at TEXT,
    prepared_with TEXT,
    prepared_texts INTEGER,
    prepared_missing INTEGER,
    -- The translation from our GitHub in use.
    pack_made TEXT,
    pack_lines INTEGER
);

-- `original` is NULL for a file or folder the launcher added.
CREATE TABLE library_game_translation_change (
    installation_id TEXT NOT NULL
        REFERENCES library_game_translation(installation_id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    original BLOB,
    written_sha256 TEXT,
    written_bytes INTEGER,
    PRIMARY KEY (installation_id, path)
);
