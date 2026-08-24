-- Current empty database schema for the first public DLA Launcher release.
-- Future releases add ordered migrations after this baseline.

CREATE TABLE catalog_active_generation (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation_id TEXT NOT NULL REFERENCES catalog_generation(generation_id)
);

CREATE TABLE catalog_generation (
    generation_id TEXT PRIMARY KEY,
    snapshot_id TEXT NOT NULL,
    generation_kind TEXT NOT NULL CHECK (generation_kind IN ('embedded', 'imported')),
    profile TEXT NOT NULL CHECK (profile IN ('compact', 'full', 'custom')),
    source_name TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    work_count INTEGER NOT NULL CHECK (work_count >= 0),
    rom_count INTEGER NOT NULL CHECK (rom_count >= 0),
    database_bytes INTEGER NOT NULL CHECK (database_bytes >= 0),
    fields_json TEXT NOT NULL CHECK (json_valid(fields_json)),
    catalog_path TEXT NOT NULL,
    failed INTEGER NOT NULL DEFAULT 0 CHECK (failed IN (0, 1)),
    failure_detail TEXT NOT NULL DEFAULT ''
, package_name TEXT NOT NULL DEFAULT '');

CREATE TABLE library_android_app_association (
    association_id TEXT PRIMARY KEY,
    work_code TEXT NOT NULL COLLATE NOCASE UNIQUE
        REFERENCES library_work(work_code) ON DELETE CASCADE,
    package_name TEXT NOT NULL UNIQUE,
    application_label TEXT NOT NULL,
    expected_signing_certificate_sha256 TEXT NOT NULL CHECK (
        json_valid(expected_signing_certificate_sha256)
        AND json_type(expected_signing_certificate_sha256) = 'array'
        AND json_array_length(expected_signing_certificate_sha256) > 0
    ),
    associated_version_name TEXT,
    associated_version_code TEXT NOT NULL,
    associated_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_launched_at TEXT,
    launch_count INTEGER NOT NULL DEFAULT 0 CHECK (launch_count >= 0)
);

CREATE TABLE library_audio_track (
    installation_id TEXT NOT NULL REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    work_code TEXT COLLATE NOCASE,
    relative_path TEXT NOT NULL CHECK (length(trim(relative_path)) > 0),
    size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    disc_number INTEGER CHECK (disc_number IS NULL OR disc_number >= 0),
    track_number INTEGER CHECK (track_number IS NULL OR track_number >= 0),
    bonus INTEGER NOT NULL CHECK (bonus IN (0, 1)),
    sort_order INTEGER NOT NULL CHECK (sort_order >= 0),
    indexed_at TEXT NOT NULL CHECK (length(trim(indexed_at)) > 0),
    PRIMARY KEY (installation_id, relative_path)
);

CREATE TABLE library_content_item (
    installation_id TEXT NOT NULL REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    path_key TEXT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('executable', 'audio', 'image', 'pdf', 'video', 'archive', 'android_package', 'directory', 'unknown')),
    size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    modified_at TEXT,
    confidence TEXT NOT NULL CHECK (confidence IN ('low', 'medium', 'high')),
    reason_codes TEXT NOT NULL CHECK (json_valid(reason_codes) AND json_type(reason_codes) = 'array' AND json_array_length(reason_codes) > 0),
    PRIMARY KEY (installation_id, path_key),
    UNIQUE (installation_id, relative_path)
);

CREATE TABLE library_content_item_override (
    installation_id TEXT NOT NULL REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    media_type TEXT CHECK (media_type IS NULL OR media_type IN ('executable', 'audio', 'image', 'pdf', 'video', 'archive', 'android_package', 'directory', 'unknown')),
    ignored INTEGER NOT NULL CHECK (ignored IN (0, 1)),
    sequence_order INTEGER CHECK (sequence_order IS NULL OR sequence_order >= 0),
    PRIMARY KEY (installation_id, relative_path),
    CHECK (media_type IS NOT NULL OR ignored = 1 OR sequence_order IS NOT NULL)
);

CREATE TABLE library_installation (
    installation_id TEXT PRIMARY KEY,
    scan_root_id TEXT REFERENCES library_scan_root(root_id) ON DELETE SET NULL,
    source_scan_session_id TEXT REFERENCES library_scan_session(session_id) ON DELETE SET NULL,
    root_path TEXT NOT NULL CHECK (length(trim(root_path)) > 0),
    platform TEXT NOT NULL CHECK (platform IN ('windows', 'linux', 'macos', 'android', 'ios', 'unknown')),
    status TEXT NOT NULL CHECK (status IN ('ready', 'needs_review')),
    work_code TEXT COLLATE NOCASE,
    identity_confidence TEXT CHECK (identity_confidence IS NULL OR identity_confidence IN ('possible', 'strong', 'exact')),
    identity_reason_codes TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(identity_reason_codes) AND json_type(identity_reason_codes) = 'array'),
    suggested_status TEXT NOT NULL CHECK (suggested_status IN ('ready', 'needs_review')),
    discovered_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        (work_code IS NULL AND identity_confidence IS NULL AND json_array_length(identity_reason_codes) = 0)
        OR
        (work_code IS NOT NULL AND identity_confidence IS NOT NULL AND json_array_length(identity_reason_codes) > 0)
    )
);

CREATE TABLE library_installation_health (
    installation_id TEXT PRIMARY KEY
        REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (
        state IN (
            'unknown', 'healthy', 'missing_files', 'modified_files', 'moved',
            'inaccessible', 'needs_review', 'repairable'
        )
    ),
    managed INTEGER NOT NULL CHECK (managed IN (0, 1)),
    repairable INTEGER NOT NULL CHECK (repairable IN (0, 1)),
    checked_root TEXT NOT NULL,
    expected_files INTEGER NOT NULL CHECK (expected_files >= 0),
    present_files INTEGER NOT NULL CHECK (present_files >= 0),
    missing_files INTEGER NOT NULL CHECK (missing_files >= 0),
    modified_files INTEGER NOT NULL CHECK (modified_files >= 0),
    inaccessible_files INTEGER NOT NULL CHECK (inaccessible_files >= 0),
    unexpected_files INTEGER NOT NULL CHECK (unexpected_files >= 0),
    issues TEXT NOT NULL,
    checked_at TEXT NOT NULL CHECK (length(trim(checked_at)) > 0)
);

CREATE TABLE library_installation_legacy_selection (
    installation_id TEXT PRIMARY KEY REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    platform TEXT NOT NULL,
    content_kind TEXT NOT NULL,
    launch_target TEXT NOT NULL
);

CREATE TABLE library_installation_override (
    installation_id TEXT PRIMARY KEY REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    identity_override_kind TEXT CHECK (identity_override_kind IS NULL OR identity_override_kind IN ('catalog_work', 'unidentified')),
    identity_work_code TEXT COLLATE NOCASE,
    custom_title TEXT,
    preferred_action_kind TEXT CHECK (preferred_action_kind IS NULL OR preferred_action_kind IN ('launch_executable', 'play_audio', 'read_images', 'open_document', 'play_video', 'open_archive', 'open_android_package')),
    preferred_target_kind TEXT CHECK (preferred_target_kind IS NULL OR preferred_target_kind IN ('installation_root', 'relative_path')),
    preferred_target_path TEXT,
    reviewed_at TEXT,
    updated_at TEXT NOT NULL,
    CHECK (
        (identity_override_kind IS NULL AND identity_work_code IS NULL)
        OR
        (identity_override_kind = 'catalog_work' AND identity_work_code IS NOT NULL AND length(trim(identity_work_code)) > 0)
        OR
        (identity_override_kind = 'unidentified' AND identity_work_code IS NULL)
    ),
    CHECK (reviewed_at IS NULL OR length(trim(reviewed_at)) > 0),
    CHECK (
        (preferred_action_kind IS NULL AND preferred_target_kind IS NULL AND preferred_target_path IS NULL)
        OR
        (preferred_action_kind IS NOT NULL AND preferred_target_kind = 'installation_root' AND preferred_target_path IS NULL)
        OR
        (preferred_action_kind IS NOT NULL AND preferred_target_kind = 'relative_path' AND preferred_target_path IS NOT NULL AND length(preferred_target_path) > 0)
    )
);

CREATE TABLE library_launch_activity (
    activity_id TEXT PRIMARY KEY CHECK (length(trim(activity_id)) > 0),
    installation_id TEXT NOT NULL REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    action_kind TEXT CHECK (
        action_kind IS NULL OR action_kind IN (
            'launch_executable', 'play_audio', 'read_images', 'open_document',
            'play_video', 'open_archive', 'open_android_package'
        )
    ),
    target_path TEXT,
    adapter TEXT CHECK (
        adapter IS NULL OR adapter IN ('windows_native', 'linux_native', 'linux_wine')
    ),
    status TEXT NOT NULL CHECK (
        status IN ('starting', 'running', 'stopping', 'exited', 'failed', 'stopped', 'interrupted')
    ),
    process_id INTEGER CHECK (process_id IS NULL OR process_id >= 0),
    error TEXT,
    attempted_at TEXT NOT NULL CHECK (length(trim(attempted_at)) > 0),
    started_at TEXT,
    ended_at TEXT,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    exit_code INTEGER,
    stop_requested_at TEXT,
    CHECK (
        (status = 'starting' AND action_kind IS NOT NULL AND target_path IS NOT NULL
            AND adapter IS NULL AND process_id IS NULL AND error IS NULL
            AND started_at IS NULL AND ended_at IS NULL AND duration_ms IS NULL
            AND exit_code IS NULL AND stop_requested_at IS NULL)
        OR
        (status = 'running' AND action_kind IS NOT NULL AND target_path IS NOT NULL
            AND adapter IS NOT NULL AND process_id IS NOT NULL AND error IS NULL
            AND started_at IS NOT NULL AND ended_at IS NULL AND duration_ms IS NULL
            AND exit_code IS NULL AND stop_requested_at IS NULL)
        OR
        (status = 'stopping' AND action_kind IS NOT NULL AND target_path IS NOT NULL
            AND adapter IS NOT NULL AND process_id IS NOT NULL AND error IS NULL
            AND started_at IS NOT NULL AND ended_at IS NULL AND duration_ms IS NULL
            AND exit_code IS NULL AND stop_requested_at IS NOT NULL)
        OR
        (status = 'exited' AND action_kind IS NOT NULL AND target_path IS NOT NULL
            AND adapter IS NOT NULL AND process_id IS NOT NULL AND error IS NULL
            AND started_at IS NOT NULL AND ended_at IS NOT NULL AND duration_ms IS NOT NULL
            AND stop_requested_at IS NULL)
        OR
        (status = 'stopped' AND action_kind IS NOT NULL AND target_path IS NOT NULL
            AND adapter IS NOT NULL AND process_id IS NOT NULL AND error IS NULL
            AND started_at IS NOT NULL AND ended_at IS NOT NULL AND duration_ms IS NOT NULL
            AND stop_requested_at IS NOT NULL)
        OR
        (status IN ('failed', 'interrupted')
            AND error IS NOT NULL AND length(trim(error)) > 0 AND ended_at IS NOT NULL)
    )
);

CREATE TABLE library_launch_candidate (
    installation_id TEXT NOT NULL REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    candidate_id TEXT NOT NULL,
    action_kind TEXT NOT NULL CHECK (action_kind IN ('launch_executable', 'play_audio', 'read_images', 'open_document', 'play_video', 'open_archive', 'open_android_package')),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('installation_root', 'relative_path')),
    target_path TEXT,
    supported_platforms TEXT NOT NULL CHECK (json_valid(supported_platforms) AND json_type(supported_platforms) = 'array' AND json_array_length(supported_platforms) > 0),
    confidence TEXT NOT NULL CHECK (confidence IN ('low', 'medium', 'high')),
    reason_codes TEXT NOT NULL CHECK (json_valid(reason_codes) AND json_type(reason_codes) = 'array' AND json_array_length(reason_codes) > 0),
    PRIMARY KEY (installation_id, candidate_id),
    CHECK (
        (target_kind = 'installation_root' AND target_path IS NULL)
        OR
        (target_kind = 'relative_path' AND target_path IS NOT NULL AND length(target_path) > 0)
    )
);

CREATE TABLE library_media_queue_state (
    scope_key TEXT PRIMARY KEY CHECK (length(trim(scope_key)) > 0),
    session_kind TEXT NOT NULL CHECK (session_kind IN ('work', 'personalized_voice')),
    installation_id TEXT,
    current_installation_id TEXT NOT NULL,
    current_relative_path TEXT NOT NULL CHECK (length(trim(current_relative_path)) > 0),
    position_ms INTEGER NOT NULL CHECK (position_ms >= 0),
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    completed INTEGER NOT NULL CHECK (completed IN (0, 1)),
    repeat_mode TEXT NOT NULL CHECK (repeat_mode IN ('off', 'all', 'one')),
    shuffle_enabled INTEGER NOT NULL CHECK (shuffle_enabled IN (0, 1)),
    updated_at TEXT NOT NULL CHECK (length(trim(updated_at)) > 0),
    CHECK (
        (session_kind = 'work' AND installation_id IS NOT NULL)
        OR (session_kind = 'personalized_voice' AND installation_id IS NULL)
    )
);

CREATE TABLE library_media_resume (
    installation_id TEXT NOT NULL REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    action_kind TEXT NOT NULL CHECK (
        action_kind IN ('play_audio', 'read_images', 'open_document', 'play_video')
    ),
    relative_path TEXT NOT NULL CHECK (length(trim(relative_path)) > 0),
    position_ms INTEGER NOT NULL CHECK (position_ms >= 0),
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    completed INTEGER NOT NULL CHECK (completed IN (0, 1)),
    updated_at TEXT NOT NULL CHECK (length(trim(updated_at)) > 0),
    PRIMARY KEY (installation_id, action_kind)
);

CREATE TABLE library_media_session (
    session_id TEXT PRIMARY KEY CHECK (length(trim(session_id)) > 0),
    installation_id TEXT NOT NULL REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    action_kind TEXT NOT NULL CHECK (
        action_kind IN ('play_audio', 'read_images', 'open_document', 'play_video')
    ),
    status TEXT NOT NULL CHECK (
        status IN ('active', 'paused', 'completed', 'closed', 'interrupted', 'failed')
    ),
    current_item_ordinal INTEGER NOT NULL CHECK (current_item_ordinal >= 0),
    position_ms INTEGER NOT NULL CHECK (position_ms >= 0),
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    completed INTEGER NOT NULL CHECK (completed IN (0, 1)),
    opened_at TEXT NOT NULL CHECK (length(trim(opened_at)) > 0),
    updated_at TEXT NOT NULL CHECK (length(trim(updated_at)) > 0),
    ended_at TEXT,
    error TEXT, session_kind TEXT NOT NULL DEFAULT 'work'
CHECK (session_kind IN ('work', 'personalized_voice')), repeat_mode TEXT NOT NULL DEFAULT 'off'
CHECK (repeat_mode IN ('off', 'all', 'one')), shuffle_enabled INTEGER NOT NULL DEFAULT 0
CHECK (shuffle_enabled IN (0, 1)),
    CHECK (
        (status = 'completed' AND completed = 1)
        OR (status <> 'completed' AND completed = 0)
    )
);

CREATE TABLE library_media_session_item (
    session_id TEXT NOT NULL REFERENCES library_media_session(session_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    relative_path TEXT NOT NULL CHECK (length(trim(relative_path)) > 0),
    media_type TEXT NOT NULL CHECK (media_type IN ('audio', 'image', 'pdf', 'video')),
    size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0), installation_id TEXT, work_code TEXT, disc_number INTEGER CHECK (disc_number IS NULL OR disc_number >= 0), track_number INTEGER CHECK (track_number IS NULL OR track_number >= 0), bonus INTEGER NOT NULL DEFAULT 0 CHECK (bonus IN (0, 1)),
    PRIMARY KEY (session_id, ordinal),
    UNIQUE (session_id, relative_path)
);

CREATE TABLE library_package_inspection (
    installation_id TEXT PRIMARY KEY REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    inspection TEXT NOT NULL CHECK (json_valid(inspection) AND json_type(inspection) = 'object'),
    inspected_at TEXT NOT NULL
);

CREATE TABLE library_prepared_package (
    installation_id TEXT PRIMARY KEY REFERENCES library_installation(installation_id) ON DELETE CASCADE,
    destination_root TEXT NOT NULL CHECK (length(trim(destination_root)) > 0),
    content_root TEXT,
    preferred_action TEXT CHECK (
        preferred_action IS NULL OR
        (json_valid(preferred_action) AND json_type(preferred_action) = 'object')
    ),
    source_set TEXT NOT NULL CHECK (json_valid(source_set) AND json_type(source_set) = 'object'),
    archive_retention TEXT NOT NULL CHECK (
        archive_retention IN ('keep', 'delete_after_verified_install')
    ),
    sources_deleted INTEGER NOT NULL CHECK (sources_deleted IN (0, 1)),
    source_cleanup_error TEXT,
    installed_file_count INTEGER NOT NULL CHECK (installed_file_count >= 0),
    installed_bytes INTEGER NOT NULL CHECK (installed_bytes >= 0),
    prepared_at TEXT NOT NULL,
    UNIQUE (destination_root)
);

CREATE TABLE library_scan_candidate (
    result_id TEXT NOT NULL REFERENCES library_scan_result(result_id) ON DELETE CASCADE,
    work_code TEXT NOT NULL COLLATE NOCASE,
    confidence TEXT NOT NULL CHECK (confidence IN ('possible', 'strong', 'exact')),
    reason_codes TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(reason_codes)),
    rank INTEGER NOT NULL CHECK (rank > 0),
    PRIMARY KEY (result_id, work_code),
    UNIQUE (result_id, rank)
);

CREATE TABLE library_scan_entry (
    entry_id TEXT PRIMARY KEY,
    root_id TEXT NOT NULL REFERENCES library_scan_root(root_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    path_key TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('file', 'directory')),
    extension TEXT NOT NULL,
    size TEXT,
    modified_at TEXT,
    presence TEXT NOT NULL CHECK (presence IN ('present', 'missing')),
    first_seen_session_id TEXT REFERENCES library_scan_session(session_id) ON DELETE SET NULL,
    last_seen_session_id TEXT REFERENCES library_scan_session(session_id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (root_id, path_key)
);

CREATE TABLE library_scan_evidence (
    evidence_id TEXT PRIMARY KEY,
    result_id TEXT NOT NULL REFERENCES library_scan_result(result_id) ON DELETE CASCADE,
    source_entry_id TEXT REFERENCES library_scan_entry(entry_id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (kind IN ('product_code', 'archive_md5', 'archive_sha1', 'archive_sha256', 'filename')),
    normalized_value TEXT NOT NULL,
    reason_code TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE library_scan_issue (
    issue_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES library_scan_session(session_id) ON DELETE CASCADE,
    entry_id TEXT REFERENCES library_scan_entry(entry_id) ON DELETE SET NULL,
    relative_path TEXT,
    code TEXT NOT NULL CHECK (code IN ('root_unavailable', 'permission_denied', 'entry_vanished', 'unsupported_entry', 'io', 'persistence')),
    message TEXT NOT NULL,
    recoverable INTEGER NOT NULL CHECK (recoverable IN (0, 1)),
    created_at TEXT NOT NULL
);

CREATE TABLE library_scan_result (
    result_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES library_scan_session(session_id) ON DELETE CASCADE,
    candidate_entry_id TEXT REFERENCES library_scan_entry(entry_id) ON DELETE SET NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('matched', 'ambiguous', 'unmatched')),
    selected_work_code TEXT COLLATE NOCASE,
    confidence TEXT CHECK (confidence IS NULL OR confidence IN ('possible', 'strong', 'exact')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        (outcome = 'matched' AND selected_work_code IS NOT NULL AND confidence IS NOT NULL)
        OR (outcome <> 'matched' AND selected_work_code IS NULL AND confidence IS NULL)
    ),
    UNIQUE (session_id, candidate_entry_id)
);

CREATE TABLE library_scan_root (
    root_id TEXT PRIMARY KEY,
    platform TEXT NOT NULL,
    path_key TEXT NOT NULL,
    display_path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (platform, path_key)
);

CREATE TABLE library_scan_session (
    session_id TEXT PRIMARY KEY,
    root_id TEXT NOT NULL REFERENCES library_scan_root(root_id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'cancelled', 'interrupted', 'failed')),
    follow_symlinks INTEGER NOT NULL CHECK (follow_symlinks IN (0, 1)),
    hash_policy TEXT NOT NULL CHECK (hash_policy = 'candidate_archives'),
    worker_limit INTEGER NOT NULL CHECK (worker_limit > 0),
    discovered_files INTEGER NOT NULL DEFAULT 0 CHECK (discovered_files >= 0),
    discovered_directories INTEGER NOT NULL DEFAULT 0 CHECK (discovered_directories >= 0),
    inspected_files INTEGER NOT NULL DEFAULT 0 CHECK (inspected_files >= 0),
    matched INTEGER NOT NULL DEFAULT 0 CHECK (matched >= 0),
    ambiguous INTEGER NOT NULL DEFAULT 0 CHECK (ambiguous >= 0),
    unmatched INTEGER NOT NULL DEFAULT 0 CHECK (unmatched >= 0),
    recoverable_errors INTEGER NOT NULL DEFAULT 0 CHECK (recoverable_errors >= 0),
    started_at TEXT NOT NULL,
    finished_at TEXT,
    fatal_error_code TEXT CHECK (fatal_error_code IS NULL OR fatal_error_code IN ('root_unavailable', 'permission_denied', 'entry_vanished', 'unsupported_entry', 'io', 'persistence')),
    fatal_error_message TEXT,
    CHECK ((fatal_error_code IS NULL) = (fatal_error_message IS NULL)),
    CHECK (status IN ('queued', 'running') OR finished_at IS NOT NULL)
);

CREATE TABLE library_scanner_preference (
    platform TEXT PRIMARY KEY,
    display_path TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE library_work (
    work_code TEXT PRIMARY KEY COLLATE NOCASE,
    marker TEXT NOT NULL DEFAULT '',
    note TEXT NOT NULL DEFAULT '',
    progress_kind TEXT NOT NULL DEFAULT '',
    progress_value REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (progress_value IS NULL OR progress_value >= 0)
);

CREATE TABLE library_work_preference (
    work_code TEXT PRIMARY KEY COLLATE NOCASE,
    preference TEXT NOT NULL CHECK (preference IN ('favorite', 'not_interested')),
    updated_at TEXT NOT NULL
);

CREATE INDEX catalog_generation_imported_at
    ON catalog_generation(imported_at DESC, generation_id);

CREATE INDEX library_android_app_association_updated
    ON library_android_app_association(updated_at DESC, association_id);

CREATE INDEX library_audio_track_work_order
    ON library_audio_track(work_code, installation_id, sort_order);

CREATE INDEX library_content_item_media ON library_content_item(installation_id, media_type, relative_path);

CREATE INDEX library_installation_health_state
    ON library_installation_health(state, checked_at DESC, installation_id);

CREATE INDEX library_installation_override_work
    ON library_installation_override(identity_work_code, installation_id)
    WHERE identity_override_kind = 'catalog_work';

CREATE INDEX library_installation_platform_status ON library_installation(platform, status);

CREATE INDEX library_installation_scan_root ON library_installation(scan_root_id, root_path);

CREATE INDEX library_installation_work ON library_installation(work_code);

CREATE INDEX library_launch_activity_installation_time
    ON library_launch_activity(installation_id, attempted_at DESC, activity_id DESC);

CREATE UNIQUE INDEX library_launch_activity_one_active_installation
    ON library_launch_activity(installation_id)
    WHERE status IN ('starting', 'running', 'stopping');

CREATE INDEX library_launch_activity_recent_time
    ON library_launch_activity(attempted_at DESC, activity_id DESC);

CREATE INDEX library_launch_candidate_action ON library_launch_candidate(installation_id, action_kind);

CREATE INDEX library_media_session_installation_time
    ON library_media_session(installation_id, updated_at DESC, session_id DESC);

CREATE UNIQUE INDEX library_media_session_one_open_installation
    ON library_media_session(installation_id)
    WHERE session_kind = 'work' AND status IN ('active', 'paused');

CREATE UNIQUE INDEX library_media_session_one_open_voice_queue
    ON library_media_session(session_kind)
    WHERE session_kind = 'personalized_voice' AND status IN ('active', 'paused');

CREATE INDEX library_media_session_recent_time
    ON library_media_session(updated_at DESC, session_id DESC);

CREATE INDEX library_package_inspection_time
    ON library_package_inspection(inspected_at DESC, installation_id);

CREATE INDEX library_prepared_package_time
    ON library_prepared_package(prepared_at DESC, installation_id);

CREATE INDEX library_scan_entry_root_presence
    ON library_scan_entry(root_id, presence, path_key);

CREATE INDEX library_scan_evidence_result
    ON library_scan_evidence(result_id, kind);

CREATE INDEX library_scan_evidence_value
    ON library_scan_evidence(kind, normalized_value);

CREATE INDEX library_scan_issue_session
    ON library_scan_issue(session_id, recoverable, issue_id);

CREATE INDEX library_scan_result_selected_work
    ON library_scan_result(selected_work_code, outcome);

CREATE INDEX library_scan_result_session_outcome
    ON library_scan_result(session_id, outcome, result_id);

CREATE INDEX library_scan_session_root_started
    ON library_scan_session(root_id, started_at DESC);

CREATE INDEX library_scan_session_status
    ON library_scan_session(status, started_at DESC);

CREATE INDEX library_work_preference_kind_updated
    ON library_work_preference(preference, updated_at DESC, work_code);
