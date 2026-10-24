-- Milestone 4.H: how Sentinal itself is doing. Numbers, closed codes, and
-- Machine IDs only (see protocol::observability). Raw samples are kept 24 h,
-- one-minute summaries and incidents 30 days.
CREATE TABLE health_samples (
    machine_id TEXT NOT NULL,
    metric TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    minute_at INTEGER NOT NULL,
    at INTEGER NOT NULL,
    value REAL NOT NULL
);
CREATE INDEX health_samples_key ON health_samples(machine_id, metric, label, minute_at);
CREATE INDEX health_samples_at ON health_samples(at);

CREATE TABLE health_minutes (
    machine_id TEXT NOT NULL,
    metric TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    minute_at INTEGER NOT NULL,
    count INTEGER NOT NULL,
    sum REAL NOT NULL,
    min REAL NOT NULL,
    max REAL NOT NULL,
    -- {"<bucket>": count}; bucket = floor(4 * log2(value + 1)).
    buckets TEXT NOT NULL,
    -- Samples refused by the per-minute cap, never silently lost.
    dropped INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (machine_id, metric, label, minute_at)
);
CREATE INDEX health_minutes_time ON health_minutes(minute_at);

CREATE TABLE health_incidents (
    id INTEGER PRIMARY KEY,
    machine_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    detail TEXT
);
CREATE INDEX health_incidents_open ON health_incidents(machine_id, kind, ended_at);
CREATE INDEX health_incidents_time ON health_incidents(started_at);
