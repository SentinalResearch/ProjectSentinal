CREATE TABLE machines (
    id TEXT PRIMARY KEY NOT NULL,
    json TEXT NOT NULL
);

CREATE TABLE sessions (
    id INTEGER PRIMARY KEY NOT NULL,
    state TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    json TEXT NOT NULL
);

CREATE TABLE commands (
    id INTEGER PRIMARY KEY NOT NULL,
    session_id INTEGER NOT NULL,
    machine_id TEXT NOT NULL,
    text TEXT NOT NULL,
    cwd TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    exit_code INTEGER,
    class TEXT
);

CREATE VIRTUAL TABLE commands_fts USING fts5(
    text,
    content='commands',
    content_rowid='id'
);

CREATE TRIGGER commands_after_insert AFTER INSERT ON commands BEGIN
    INSERT INTO commands_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER commands_after_delete AFTER DELETE ON commands BEGIN
    INSERT INTO commands_fts(commands_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;

CREATE TRIGGER commands_after_update AFTER UPDATE OF text ON commands BEGIN
    INSERT INTO commands_fts(commands_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO commands_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TABLE events (
    id INTEGER PRIMARY KEY NOT NULL,
    session_id INTEGER NOT NULL,
    command_id INTEGER,
    at INTEGER NOT NULL,
    kind_json TEXT NOT NULL
);

CREATE TABLE metrics_raw (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope_key TEXT NOT NULL,
    session_id INTEGER,
    at INTEGER NOT NULL,
    cpu_percent REAL NOT NULL,
    ram_used_bytes INTEGER NOT NULL,
    ram_total_bytes INTEGER NOT NULL,
    gpu_percent REAL,
    vram_used_bytes INTEGER,
    vram_total_bytes INTEGER
);

CREATE INDEX metrics_raw_at ON metrics_raw(at);

CREATE TABLE metrics_minute (
    scope_key TEXT NOT NULL,
    session_id INTEGER,
    minute_at INTEGER NOT NULL,
    cpu_percent REAL NOT NULL,
    ram_used_bytes INTEGER NOT NULL,
    ram_total_bytes INTEGER NOT NULL,
    gpu_percent REAL,
    vram_used_bytes INTEGER,
    vram_total_bytes INTEGER,
    sample_count INTEGER NOT NULL,
    PRIMARY KEY (scope_key, minute_at)
);
