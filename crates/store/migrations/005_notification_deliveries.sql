CREATE TABLE notification_deliveries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id INTEGER NOT NULL,
    machine_id TEXT NOT NULL,
    session_id INTEGER NOT NULL,
    category TEXT NOT NULL,
    state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER,
    result_class TEXT,
    request_bytes INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(event_id, category),
    FOREIGN KEY(event_id) REFERENCES events(id)
);

CREATE INDEX notification_deliveries_due
    ON notification_deliveries(state, next_attempt_at, id);

CREATE INDEX notification_deliveries_dedup
    ON notification_deliveries(machine_id, session_id, category, created_at);
