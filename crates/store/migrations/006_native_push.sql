CREATE TABLE notification_intent_outbox (
    intent_id TEXT PRIMARY KEY NOT NULL,
    machine_id TEXT NOT NULL,
    event_id INTEGER NOT NULL,
    session_id INTEGER NOT NULL,
    occurred_at INTEGER NOT NULL,
    session_label TEXT,
    facts_json TEXT NOT NULL,
    encoded_bytes INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    last_result TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX notification_intent_outbox_due
    ON notification_intent_outbox(next_attempt_at, occurred_at, event_id);

CREATE TABLE control_notification_intents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    machine_id TEXT NOT NULL,
    intent_id TEXT NOT NULL,
    event_id INTEGER NOT NULL,
    session_id INTEGER NOT NULL,
    occurred_at INTEGER NOT NULL,
    session_label TEXT,
    facts_json TEXT NOT NULL,
    category TEXT NOT NULL,
    decision TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(machine_id, intent_id)
);

CREATE INDEX control_notification_intents_dedup
    ON control_notification_intents(machine_id, session_id, category, created_at);

CREATE TABLE push_subscriptions (
    subscription_id TEXT PRIMARY KEY NOT NULL,
    endpoint_hash TEXT NOT NULL UNIQUE,
    endpoint TEXT NOT NULL,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    device_label TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_success_at INTEGER,
    expiration_time INTEGER
);

CREATE INDEX push_subscriptions_state
    ON push_subscriptions(state, subscription_id);

CREATE TABLE push_deliveries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    control_intent_id INTEGER NOT NULL,
    machine_id TEXT NOT NULL,
    session_id INTEGER NOT NULL,
    subscription_id TEXT NOT NULL,
    category TEXT NOT NULL,
    state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER,
    result_class TEXT,
    request_bytes INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(control_intent_id, subscription_id),
    FOREIGN KEY(control_intent_id) REFERENCES control_notification_intents(id),
    FOREIGN KEY(subscription_id) REFERENCES push_subscriptions(subscription_id)
);

CREATE INDEX push_deliveries_due
    ON push_deliveries(state, next_attempt_at, id);

CREATE TABLE notification_provider_state (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    mode TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO notification_provider_state(singleton, mode, updated_at)
VALUES (1, 'DISABLED', 0);
