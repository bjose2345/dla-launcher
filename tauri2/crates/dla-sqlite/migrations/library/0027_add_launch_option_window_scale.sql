-- How Linux shows the game's window: NULL as the game made it, 'fit' or
-- 'integer' enlarged to the screen through gamescope.
ALTER TABLE library_launch_option ADD COLUMN window_scale TEXT;
