-- For a game enlarged through gamescope: whether MangoHud's performance
-- overlay is drawn over it, and after how many milliseconds without movement
-- the cursor is hidden; NULL leaves gamescope's ten seconds.
ALTER TABLE library_launch_option ADD COLUMN performance_overlay INTEGER NOT NULL DEFAULT 0;
ALTER TABLE library_launch_option ADD COLUMN cursor_hide_delay INTEGER;
