-- A file of a game the launcher took out of an executable that holds the
-- game's files packed inside it: `unpacked_from` is that executable, and
-- `written_sha256` and `written_bytes` are the file as it was written.
-- `unpacked_list` is set on the program that starts in place of the
-- executable: the SHA-256 of the list of files the executable held.
ALTER TABLE library_game_translation_change ADD COLUMN unpacked_from TEXT;
ALTER TABLE library_game_translation_change ADD COLUMN unpacked_list TEXT;
