CREATE TABLE translation_speaker (
    work TEXT NOT NULL,
    language TEXT NOT NULL,
    map_id INTEGER NOT NULL,
    event_id INTEGER NOT NULL,
    page_index INTEGER NOT NULL,
    command_index INTEGER NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('edit', 'pack')),
    key TEXT NOT NULL,
    expected_speaker TEXT NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY (work, language, map_id, event_id, page_index, command_index, origin)
) WITHOUT ROWID;
