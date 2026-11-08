-- An audio player program of the user's for the work's tracks; without one
-- the launcher's own player plays them.
ALTER TABLE library_launch_option ADD COLUMN audio_player TEXT;
