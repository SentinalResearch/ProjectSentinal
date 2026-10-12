-- Click-to-approve pairing requests (4.E §6). Secrets never live here: only
-- the SHA-256 of the worker's request secret, and public nonces.
CREATE TABLE pairing_requests (
    request_id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    state TEXT NOT NULL,
    secret_sha256 TEXT NOT NULL,
    worker_nonce TEXT NOT NULL,
    control_nonce TEXT NOT NULL,
    claimed_machine_json TEXT NOT NULL,
    endpoint_json TEXT NOT NULL,
    tailscale_json TEXT,
    proof_ok INTEGER NOT NULL DEFAULT 0,
    approved_name TEXT,
    decided_at INTEGER,
    claimed_at INTEGER
);

CREATE INDEX pairing_requests_created ON pairing_requests(created_at);

CREATE TABLE pairing_claim_failures (at INTEGER NOT NULL);
