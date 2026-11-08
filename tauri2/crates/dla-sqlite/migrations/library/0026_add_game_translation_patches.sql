-- A game file the launcher changed in place instead of keeping a copy (an
-- RPG Maker VX Ace archive): `original` holds the bytes it overwrote at
-- `patched_at`, and the file was `original_bytes` long before.
ALTER TABLE library_game_translation_change ADD COLUMN patched_at INTEGER;
ALTER TABLE library_game_translation_change ADD COLUMN original_bytes INTEGER;
