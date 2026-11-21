-- A game file or folder the launcher moved aside untouched to put its own
-- in its place (a WOLF RPG Editor data archive): `moved_to` is where the
-- game's own now is.
ALTER TABLE library_game_translation_change ADD COLUMN moved_to TEXT;
