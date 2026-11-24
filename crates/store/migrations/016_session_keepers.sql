-- Keepers (T5, docs/R1_KEEPER_DESIGN.md §8): what a restarted daemon needs to
-- reclaim the process holding a Session's terminal. `secret_hex` never leaves
-- this private database and is never logged.
CREATE TABLE session_keepers (
    session_id INTEGER PRIMARY KEY NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    socket_path TEXT NOT NULL,
    secret_hex TEXT NOT NULL,
    osc_nonce TEXT NOT NULL,
    keeper_pid INTEGER NOT NULL,
    keeper_started_at INTEGER NOT NULL,
    shell_pid INTEGER NOT NULL,
    shell_started_at INTEGER NOT NULL,
    protocol INTEGER NOT NULL,
    scope_unit TEXT,
    created_at INTEGER NOT NULL
);
