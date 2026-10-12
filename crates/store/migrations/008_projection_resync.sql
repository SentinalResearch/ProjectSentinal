-- Durable current worker projection and staged multi-chunk control snapshots.
CREATE TABLE worker_projection_records (
    machine_id TEXT NOT NULL,
    source_epoch TEXT NOT NULL,
    record_key TEXT NOT NULL,
    source_cursor INTEGER NOT NULL,
    change_json TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    PRIMARY KEY(machine_id, source_epoch, record_key)
);

CREATE INDEX worker_projection_records_cursor
    ON worker_projection_records(machine_id, source_epoch, source_cursor);

CREATE TABLE control_projection_snapshot_chunks (
    machine_id TEXT NOT NULL,
    source_epoch TEXT NOT NULL,
    snapshot_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    snapshot_cursor INTEGER NOT NULL,
    records_json TEXT NOT NULL,
    sent_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(machine_id, source_epoch, snapshot_id, chunk_index)
);
