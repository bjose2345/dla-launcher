CREATE TABLE translation_picture (
    work TEXT NOT NULL,
    language TEXT NOT NULL,
    path TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('edit', 'pack')),
    png BLOB NOT NULL,
    PRIMARY KEY (work, language, path, origin)
);
