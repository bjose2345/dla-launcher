CREATE INDEX catalog_work_activity_month_browse
    ON catalog_work(max(added_date, updated_date) DESC, work_code DESC);
