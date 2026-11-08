-- Game text the player's games showed, keyed per work by a hash of the exact
-- Japanese. The Japanese stays on this PC; shared packs carry only the key.
CREATE TABLE translation_text (
  work TEXT NOT NULL,
  key TEXT NOT NULL,
  source TEXT NOT NULL,
  kind TEXT NOT NULL,
  PRIMARY KEY (work, key)
) WITHOUT ROWID;

-- One row per origin of a translation (the player's edit, a pack, the
-- player's translator); readers pick the most trusted one.
CREATE TABLE translation (
  work TEXT NOT NULL,
  key TEXT NOT NULL,
  language TEXT NOT NULL,
  origin TEXT NOT NULL,
  text TEXT NOT NULL,
  pack TEXT,
  model TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (work, key, language, origin),
  FOREIGN KEY (work, key) REFERENCES translation_text (work, key) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE translation_name (
  work TEXT NOT NULL,
  japanese TEXT NOT NULL,
  language TEXT NOT NULL,
  text TEXT NOT NULL,
  PRIMARY KEY (work, japanese, language)
) WITHOUT ROWID;
