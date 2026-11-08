-- A video player program of the user's for the work's videos; without one
-- the launcher's own player plays them.
ALTER TABLE library_launch_option ADD COLUMN video_player TEXT;
