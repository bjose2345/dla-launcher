CREATE TABLE library_delivery_job (
    job_id TEXT PRIMARY KEY,
    work_code TEXT NOT NULL COLLATE NOCASE,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    job_json TEXT NOT NULL CHECK (json_valid(job_json))
);

CREATE INDEX library_delivery_job_created
    ON library_delivery_job(created_at, job_id);

CREATE TABLE library_install_folder_preference (
    platform TEXT PRIMARY KEY,
    display_path TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
