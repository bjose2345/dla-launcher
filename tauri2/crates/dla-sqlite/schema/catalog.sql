-- Current empty database schema for the first public DLA Launcher release.
-- Future releases add ordered migrations after this baseline.

CREATE TABLE catalog_category (
    category_code TEXT PRIMARY KEY COLLATE NOCASE,
    name TEXT NOT NULL,
    name_english TEXT NOT NULL DEFAULT ''
);

CREATE TABLE catalog_circle (
    circle_id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_english TEXT NOT NULL DEFAULT '',
    UNIQUE (name, name_english)
);

CREATE TABLE catalog_field_presence (
    field_id TEXT PRIMARY KEY
);

CREATE TABLE catalog_file_format (
    file_format_code TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    name_english TEXT NOT NULL
);

CREATE TABLE catalog_import_metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    package_format_version INTEGER NOT NULL,
    profile TEXT NOT NULL,
    source_id TEXT NOT NULL,
    source_name TEXT NOT NULL,
    manifest_json TEXT NOT NULL CHECK (json_valid(manifest_json))
);

CREATE TABLE catalog_language (
    language_code TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    name_english TEXT NOT NULL
);

CREATE TABLE catalog_miscellany (
    miscellany_code TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    name_english TEXT NOT NULL
);

CREATE TABLE catalog_relation_type (
    relation_type_code TEXT PRIMARY KEY,
    label TEXT NOT NULL
);

CREATE TABLE catalog_rom (
    work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    size TEXT NOT NULL,
    crc TEXT NOT NULL,
    md5 TEXT NOT NULL,
    sha1 TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    file_count INTEGER,
    update_date TEXT NOT NULL,
    version TEXT NOT NULL,
    PRIMARY KEY (work_code, position)
);

CREATE TABLE catalog_rom_content_entry (
    work_code TEXT NOT NULL,
    rom_position INTEGER NOT NULL,
    entry_index INTEGER NOT NULL,
    path TEXT NOT NULL,
    extension TEXT NOT NULL,
    is_directory INTEGER NOT NULL,
    size TEXT,
    crc32 TEXT NOT NULL,
    md5 TEXT NOT NULL,
    sha1 TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    hash_status TEXT NOT NULL,
    PRIMARY KEY (work_code, rom_position, entry_index),
    FOREIGN KEY (work_code, rom_position)
        REFERENCES catalog_rom_content_scan(work_code, rom_position)
        ON DELETE CASCADE
);

CREATE TABLE catalog_rom_content_scan (
    work_code TEXT NOT NULL,
    rom_position INTEGER NOT NULL,
    status TEXT NOT NULL,
    archive_format TEXT NOT NULL,
    entry_count INTEGER,
    total_uncompressed_size TEXT,
    truncated INTEGER NOT NULL,
    PRIMARY KEY (work_code, rom_position),
    FOREIGN KEY (work_code, rom_position)
        REFERENCES catalog_rom(work_code, position)
        ON DELETE CASCADE
);

CREATE TABLE catalog_snapshot (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    snapshot_id TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    real_work_count INTEGER NOT NULL,
    synthetic_work_count INTEGER NOT NULL,
    imported_at TEXT NOT NULL
);

CREATE TABLE catalog_tag (
    tag_id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_english TEXT NOT NULL DEFAULT '',
    UNIQUE (name, name_english)
);

CREATE TABLE catalog_work (
    work_code TEXT PRIMARY KEY COLLATE NOCASE,
    source_code TEXT NOT NULL,
    title TEXT NOT NULL,
    title_english TEXT NOT NULL DEFAULT '',
    release_date TEXT NOT NULL DEFAULT '',
    age_rating TEXT NOT NULL DEFAULT '',
    release_type TEXT NOT NULL DEFAULT '',
    thumbnail_urls TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(thumbnail_urls)),
    is_synthetic INTEGER NOT NULL DEFAULT 0 CHECK (is_synthetic IN (0, 1))
, added_date TEXT NOT NULL DEFAULT '', updated_date TEXT NOT NULL DEFAULT '', main_image_urls TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(main_image_urls)), sample_image_urls TEXT NOT NULL DEFAULT '[]', rating_score REAL, rating_count INTEGER, total_sales INTEGER, rating_rankings TEXT NOT NULL DEFAULT '[]');

CREATE TABLE catalog_work_category (
    work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    category_code TEXT NOT NULL REFERENCES catalog_category(category_code) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (work_code, category_code)
);

CREATE TABLE catalog_work_circle (
    work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    circle_id INTEGER NOT NULL REFERENCES catalog_circle(circle_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (work_code, circle_id)
);

CREATE TABLE catalog_work_enrichment (
    work_code TEXT PRIMARY KEY REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    site_name TEXT,
    drm_values TEXT CHECK (drm_values IS NULL OR json_valid(drm_values)),
    title_kana TEXT,
    title_romaji TEXT,
    source_url TEXT,
    description_versions TEXT CHECK (description_versions IS NULL OR json_valid(description_versions)),
    favorites_count INTEGER,
    raw_fields TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(raw_fields))
);

CREATE TABLE catalog_work_file_format (
    work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    file_format_code TEXT NOT NULL REFERENCES catalog_file_format(file_format_code),
    position INTEGER NOT NULL,
    PRIMARY KEY (work_code, file_format_code)
);

CREATE TABLE catalog_work_language (
    work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    language_code TEXT NOT NULL REFERENCES catalog_language(language_code),
    position INTEGER NOT NULL,
    PRIMARY KEY (work_code, language_code)
);

CREATE TABLE catalog_work_miscellany (
    work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    miscellany_code TEXT NOT NULL REFERENCES catalog_miscellany(miscellany_code),
    position INTEGER NOT NULL,
    PRIMARY KEY (work_code, miscellany_code)
);

CREATE TABLE catalog_work_relation (
    parent_work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    child_work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    relation_type_code TEXT NOT NULL REFERENCES catalog_relation_type(relation_type_code),
    PRIMARY KEY (parent_work_code, child_work_code, relation_type_code),
    CHECK (parent_work_code <> child_work_code)
);

CREATE TABLE catalog_work_tag (
    work_code TEXT NOT NULL REFERENCES catalog_work(work_code) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES catalog_tag(tag_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (work_code, tag_id)
);

CREATE INDEX catalog_rom_content_entry_path
    ON catalog_rom_content_entry(work_code, rom_position, path);

CREATE INDEX catalog_rom_md5_identity
    ON catalog_rom(md5 COLLATE NOCASE, work_code COLLATE NOCASE);

CREATE INDEX catalog_rom_sha1_identity
    ON catalog_rom(sha1 COLLATE NOCASE, work_code COLLATE NOCASE);

CREATE INDEX catalog_rom_sha256_identity
    ON catalog_rom(sha256 COLLATE NOCASE, work_code COLLATE NOCASE);

CREATE INDEX catalog_rom_size_identity
    ON catalog_rom(size, work_code COLLATE NOCASE, position);

CREATE INDEX catalog_work_added_date ON catalog_work(added_date DESC, work_code);

CREATE INDEX catalog_work_added_month_browse
    ON catalog_work(added_date DESC, work_code DESC);

CREATE INDEX catalog_work_age_rating
    ON catalog_work(age_rating, work_code);

CREATE INDEX catalog_work_category_category ON catalog_work_category(category_code, work_code);

CREATE INDEX catalog_work_circle_circle ON catalog_work_circle(circle_id, work_code);

CREATE INDEX catalog_work_file_format_value
    ON catalog_work_file_format(file_format_code, work_code);

CREATE INDEX catalog_work_language_value
    ON catalog_work_language(language_code, work_code);

CREATE INDEX catalog_work_miscellany_value
    ON catalog_work_miscellany(miscellany_code, work_code);

CREATE INDEX catalog_work_relation_child
    ON catalog_work_relation(child_work_code, parent_work_code);

CREATE INDEX catalog_work_release_date ON catalog_work(release_date DESC, work_code);

CREATE INDEX catalog_work_release_month_browse
    ON catalog_work(release_date DESC, work_code DESC);

CREATE INDEX catalog_work_source ON catalog_work(source_code, work_code);

CREATE INDEX catalog_work_tag_tag ON catalog_work_tag(tag_id, work_code);

CREATE INDEX catalog_work_title ON catalog_work(title COLLATE NOCASE, work_code);

CREATE INDEX catalog_work_updated_date ON catalog_work(updated_date DESC, work_code);

CREATE INDEX catalog_work_updated_month_browse
    ON catalog_work(updated_date DESC, work_code DESC);
