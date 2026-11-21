-- How gamescope draws an enlarged game's picture: `smooth`, or `fsr-` or
-- `nis-` with `soft`, `medium` or `strong`; NULL is sharp pixels.
ALTER TABLE library_launch_option ADD COLUMN picture_filter TEXT;
