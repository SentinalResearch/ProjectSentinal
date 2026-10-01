-- Resumable read positions for adopted Session log tails (daemon-internal).
CREATE TABLE log_cursors (
    session_id INTEGER PRIMARY KEY NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    device INTEGER NOT NULL,
    inode INTEGER NOT NULL,
    offset INTEGER NOT NULL
);
