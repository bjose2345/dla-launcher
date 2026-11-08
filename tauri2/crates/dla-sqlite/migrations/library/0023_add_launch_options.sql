-- How a work starts, beyond its start file: for now whether Windows runs it
-- in a Japanese locale, through Locale Remulator. A work without a row starts
-- as it is.
CREATE TABLE library_launch_option (
    installation_id TEXT PRIMARY KEY
        REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    japanese_locale INTEGER NOT NULL DEFAULT 0
);
