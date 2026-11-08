-- A work's glossary: what its translator is told about the game (the
-- original a parody follows) and the words it writes one way.
CREATE TABLE translation_note (
  work TEXT NOT NULL,
  language TEXT NOT NULL,
  text TEXT NOT NULL,
  PRIMARY KEY (work, language)
) WITHOUT ROWID;

CREATE TABLE translation_term (
  work TEXT NOT NULL,
  japanese TEXT NOT NULL,
  language TEXT NOT NULL,
  text TEXT NOT NULL,
  PRIMARY KEY (work, japanese, language)
) WITHOUT ROWID;
