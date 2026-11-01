-- Milestone 5.S: indexed substring search. Trigram indexes find any three or
-- more characters inside Command text and inside the text-bearing Events
-- (errors, warnings, prompts, checkpoints, servers). The unicode61
-- commands_fts index from 001 still serves one- and two-character prefixes.
CREATE VIRTUAL TABLE commands_trigram USING fts5(
    text,
    content='commands',
    content_rowid='id',
    tokenize='trigram'
);
INSERT INTO commands_trigram(commands_trigram) VALUES ('rebuild');

CREATE TRIGGER commands_trigram_after_insert AFTER INSERT ON commands BEGIN
    INSERT INTO commands_trigram(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER commands_trigram_after_delete AFTER DELETE ON commands BEGIN
    INSERT INTO commands_trigram(commands_trigram, rowid, text) VALUES ('delete', old.id, old.text);
END;

CREATE TRIGGER commands_trigram_after_update AFTER UPDATE OF text ON commands BEGIN
    INSERT INTO commands_trigram(commands_trigram, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO commands_trigram(rowid, text) VALUES (new.id, new.text);
END;

-- Contentless: the text lives in events.kind_json; rows map by Event ID.
CREATE VIRTUAL TABLE events_trigram USING fts5(
    text,
    content='',
    contentless_delete=1,
    tokenize='trigram'
);

INSERT INTO events_trigram(rowid, text)
SELECT id, CASE json_extract(kind_json, '$.type')
        WHEN 'ERROR_DETECTED' THEN json_extract(kind_json, '$.message')
        WHEN 'WARNING_DETECTED' THEN json_extract(kind_json, '$.message')
        WHEN 'WAITING_FOR_INPUT' THEN json_extract(kind_json, '$.prompt')
        WHEN 'CHECKPOINT_CREATED' THEN json_extract(kind_json, '$.path')
        WHEN 'SERVER_STARTED' THEN json_extract(kind_json, '$.url')
    END AS text
FROM events
WHERE text IS NOT NULL;

CREATE TRIGGER events_trigram_after_insert AFTER INSERT ON events BEGIN
    INSERT INTO events_trigram(rowid, text)
    SELECT new.id, text FROM (
        SELECT CASE json_extract(new.kind_json, '$.type')
            WHEN 'ERROR_DETECTED' THEN json_extract(new.kind_json, '$.message')
            WHEN 'WARNING_DETECTED' THEN json_extract(new.kind_json, '$.message')
            WHEN 'WAITING_FOR_INPUT' THEN json_extract(new.kind_json, '$.prompt')
            WHEN 'CHECKPOINT_CREATED' THEN json_extract(new.kind_json, '$.path')
            WHEN 'SERVER_STARTED' THEN json_extract(new.kind_json, '$.url')
        END AS text
    ) WHERE text IS NOT NULL;
END;

CREATE TRIGGER events_trigram_after_delete AFTER DELETE ON events BEGIN
    DELETE FROM events_trigram WHERE rowid = old.id;
END;
