-- Privacy-minimized worker outbox and control dashboard read model (4.D.1).
CREATE TABLE projection_worker_state (
    machine_id TEXT PRIMARY KEY NOT NULL,
    source_epoch TEXT NOT NULL,
    current_cursor INTEGER NOT NULL DEFAULT 0,
    acknowledged_cursor INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);

CREATE TABLE projection_outbox (
    machine_id TEXT NOT NULL,
    source_epoch TEXT NOT NULL,
    cursor INTEGER NOT NULL,
    change_json TEXT NOT NULL,
    encoded_bytes INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(machine_id, source_epoch, cursor)
);

CREATE INDEX projection_outbox_pending
    ON projection_outbox(machine_id, source_epoch, cursor);

CREATE TABLE control_projection_state (
    machine_id TEXT PRIMARY KEY NOT NULL,
    source_epoch TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    committed_cursor INTEGER NOT NULL DEFAULT 0,
    observed_at INTEGER,
    synced_at INTEGER NOT NULL,
    view_revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE control_projection_sessions (
    machine_id TEXT NOT NULL,
    session_id INTEGER NOT NULL,
    session_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(machine_id, session_id)
);

CREATE INDEX control_projection_sessions_updated
    ON control_projection_sessions(machine_id, updated_at DESC, session_id DESC);

CREATE TABLE control_projection_events (
    machine_id TEXT NOT NULL,
    event_id INTEGER NOT NULL,
    session_id INTEGER NOT NULL,
    at INTEGER NOT NULL,
    event_json TEXT NOT NULL,
    PRIMARY KEY(machine_id, event_id)
);

CREATE INDEX control_projection_events_recent
    ON control_projection_events(machine_id, at DESC, event_id DESC);

CREATE TABLE control_projection_metrics (
    machine_id TEXT NOT NULL,
    scope TEXT NOT NULL,
    session_key INTEGER NOT NULL,
    metrics_json TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY(machine_id, scope, session_key)
);

CREATE TABLE control_projection_changes (
    revision INTEGER PRIMARY KEY AUTOINCREMENT,
    machine_id TEXT NOT NULL,
    change_json TEXT NOT NULL,
    committed_at INTEGER NOT NULL
);

CREATE INDEX control_projection_changes_machine
    ON control_projection_changes(machine_id, revision);
