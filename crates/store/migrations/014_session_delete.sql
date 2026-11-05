-- Permanent Session deletion: IDs must never be handed out again, or a new
-- Session could inherit an old one's URLs, notifications and control history.
CREATE TABLE id_floors (
    name TEXT PRIMARY KEY NOT NULL,
    value INTEGER NOT NULL
);
