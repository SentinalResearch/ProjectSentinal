-- Control-Machine registry, one-time pairing codes, and worker links (4.0).
-- Secrets never live here: only SHA-256 credential hashes and keyed code HMACs.
CREATE TABLE registry_machines (
    machine_id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL,
    os TEXT,
    daemon_version TEXT,
    capabilities_json TEXT NOT NULL DEFAULT '[]',
    endpoint_json TEXT,
    state TEXT NOT NULL,
    paired_at INTEGER NOT NULL,
    last_seen INTEGER,
    last_sequence INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE registry_links (
    link_id TEXT PRIMARY KEY NOT NULL,
    machine_id TEXT NOT NULL REFERENCES registry_machines(machine_id),
    worker_credential_sha256 TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    previous_sha256 TEXT,
    previous_valid_until INTEGER,
    revoked_at INTEGER,
    last_used_at INTEGER
);

CREATE INDEX registry_links_machine ON registry_links(machine_id);

CREATE TABLE pairing_codes (
    code_id TEXT PRIMARY KEY NOT NULL,
    code_hmac TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER,
    cancelled_at INTEGER,
    machine_id TEXT,
    expiry_audited_at INTEGER
);

CREATE TABLE pairing_failures (at INTEGER NOT NULL);
