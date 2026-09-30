ALTER TABLE sessions ADD COLUMN continued_from INTEGER
    REFERENCES sessions(id) ON DELETE RESTRICT
    CHECK (continued_from IS NULL OR continued_from <> id);

CREATE UNIQUE INDEX sessions_one_successor
    ON sessions(continued_from)
    WHERE continued_from IS NOT NULL;
