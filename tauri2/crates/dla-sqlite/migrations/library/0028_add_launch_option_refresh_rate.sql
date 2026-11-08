-- The refresh rate gamescope paces an enlarged game at, in Hz; NULL is the
-- launcher's default.
ALTER TABLE library_launch_option ADD COLUMN refresh_rate INTEGER;
