-- An overflowed worker delta queue must recover with a complete snapshot.
ALTER TABLE projection_worker_state
    ADD COLUMN snapshot_required INTEGER NOT NULL DEFAULT 0
    CHECK(snapshot_required IN (0, 1));
