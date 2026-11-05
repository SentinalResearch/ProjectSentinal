-- Which daemon install speaks for each Machine, and which build it runs. Two
-- installs sharing a Machine's credentials (a sandbox, a copied config) must not
-- overwrite each other's Sessions or address.
CREATE TABLE machine_instances (
    machine_id TEXT PRIMARY KEY NOT NULL,
    instance_id TEXT,
    build_commit TEXT,
    conflict_at INTEGER,
    conflict_version TEXT,
    conflict_commit TEXT,
    conflict_port INTEGER
);
