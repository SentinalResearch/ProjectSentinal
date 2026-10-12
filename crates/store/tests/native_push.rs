//! Persistent worker-outbox, subscription, and control-delivery tests.

#![allow(clippy::unwrap_used)]

use protocol::{
    EventId, MachineId, NotificationFacts, NotificationIntent, NotificationIntentBatch,
    PushProviderMode, PushSubscriptionKeys, PushSubscriptionRequest, PushSubscriptionState,
    Resource, SessionId, Timestamp,
};
use store::{NewNotificationIntentOutbox, Store};

fn intent(id: char, event: u64, session: u64, at: i64) -> NotificationIntent {
    NotificationIntent {
        intent_id: format!("ni_{}", id.to_string().repeat(64)),
        event_id: EventId(event),
        session_id: SessionId(session),
        occurred_at: Timestamp(at),
        session_label: Some("Training".into()),
        facts: NotificationFacts::ResourceHigh {
            resource: Resource::Gpu,
            percent: 96.0,
        },
    }
}

fn subscription(label: &str, endpoint: &str, auth: &str) -> PushSubscriptionRequest {
    PushSubscriptionRequest {
        device_label: label.into(),
        endpoint: endpoint.into(),
        expiration_time: None,
        keys: PushSubscriptionKeys {
            p256dh: "BBO8P4XQnN8nJzC5xwLZW7SkHV4gTBS3H3Yc4XAv9kvfnK_wvVdLR0HiMyPmVL7XdnO2jLB6zM7DczAELuKQzYU".into(),
            auth: auth.into(),
        },
    }
}

#[test]
fn native_push_tables_survive_later_migrations_without_removing_ntfy_history() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.schema_version().unwrap() >= 6);
    for table in [
        "notification_intent_outbox",
        "control_notification_intents",
        "push_subscriptions",
        "push_deliveries",
        "notification_provider_state",
        "notification_deliveries",
    ] {
        assert!(store.has_table(table).unwrap(), "missing {table}");
    }
}

#[test]
fn worker_outbox_is_ordered_idempotent_and_acknowledged() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId("01J9Z3K8V6Q2T7M4X5C0RNB8DA".into());
    for value in [intent('b', 2, 7, 2_000), intent('a', 1, 7, 1_000)] {
        store
            .record_notification_intent_outbox(&NewNotificationIntentOutbox {
                machine_id: machine.clone(),
                intent: value,
                at: Timestamp(3_000),
                pending_limit: 256,
                pending_bytes_limit: 512 * 1024,
            })
            .unwrap();
    }
    let duplicate = store
        .record_notification_intent_outbox(&NewNotificationIntentOutbox {
            machine_id: machine,
            intent: intent('a', 1, 7, 1_000),
            at: Timestamp(4_000),
            pending_limit: 256,
            pending_bytes_limit: 512 * 1024,
        })
        .unwrap();
    assert!(!duplicate.inserted);

    let due = store
        .due_notification_intents(Timestamp(4_000), 32)
        .unwrap();
    assert_eq!(due.len(), 2);
    assert_eq!(due[0].intent.event_id, EventId(1));
    assert_eq!(due[1].intent.event_id, EventId(2));
    store
        .acknowledge_notification_intents(&[due[0].intent.intent_id.clone()])
        .unwrap();
    assert_eq!(
        store
            .due_notification_intents(Timestamp(4_000), 32)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn subscriptions_upsert_by_endpoint_without_exposing_secrets() {
    let store = Store::open_in_memory().unwrap();
    let first = store
        .upsert_push_subscription(
            "ps_first",
            "endpoint-hash",
            &subscription(
                "Phone",
                "https://push.example.invalid/a",
                "AAECAwQFBgcICQoLDA0ODw",
            ),
            Timestamp(1_000),
            16,
        )
        .unwrap();
    let updated = store
        .upsert_push_subscription(
            "ps_should_not_replace",
            "endpoint-hash",
            &subscription(
                "Phone renamed",
                "https://push.example.invalid/a",
                "AQECAwQFBgcICQoLDA0ODw",
            ),
            Timestamp(2_000),
            16,
        )
        .unwrap();
    assert_eq!(updated.info.subscription_id, first.info.subscription_id);
    assert_eq!(updated.info.device_label, "Phone renamed");
    assert_eq!(updated.info.state, PushSubscriptionState::Active);
    assert_eq!(store.push_subscriptions().unwrap().len(), 1);
    assert_eq!(
        store
            .push_subscription_secret(&first.info.subscription_id)
            .unwrap()
            .unwrap()
            .keys
            .auth,
        "AQECAwQFBgcICQoLDA0ODw"
    );

    let encoded = serde_json::to_string(&store.push_subscription_infos().unwrap()).unwrap();
    assert!(!encoded.contains("push.example"));
    assert!(!encoded.contains("AQECAw"));
}

#[test]
fn device_limit_and_independent_disable_are_enforced() {
    let store = Store::open_in_memory().unwrap();
    let first = store
        .upsert_push_subscription(
            "ps_one",
            "hash-one",
            &subscription(
                "Phone",
                "https://push.example.invalid/a",
                "AAECAwQFBgcICQoLDA0ODw",
            ),
            Timestamp(1_000),
            1,
        )
        .unwrap();
    assert!(store
        .upsert_push_subscription(
            "ps_two",
            "hash-two",
            &subscription(
                "Laptop",
                "https://push.example.invalid/b",
                "AQECAwQFBgcICQoLDA0ODw",
            ),
            Timestamp(2_000),
            1,
        )
        .is_err());
    store
        .set_push_subscription_state(
            &first.info.subscription_id,
            PushSubscriptionState::Expired,
            Timestamp(3_000),
        )
        .unwrap();
    assert_eq!(store.active_push_subscriptions().unwrap().len(), 0);
}

#[test]
fn control_acceptance_is_atomic_and_idempotent_per_device() {
    let store = Store::open_in_memory().unwrap();
    store
        .upsert_push_subscription(
            "ps_one",
            "hash-one",
            &subscription(
                "Phone",
                "https://push.example.invalid/a",
                "AAECAwQFBgcICQoLDA0ODw",
            ),
            Timestamp(1_000),
            16,
        )
        .unwrap();
    let batch = NotificationIntentBatch {
        machine_id: MachineId("01J9Z3K8V6Q2T7M4X5C0RNB8DA".into()),
        sent_at: Timestamp(2_000),
        intents: vec![intent('a', 1, 7, 1_900)],
    };
    let first = store
        .accept_notification_intents(&batch, Timestamp(2_000), 7 * 86_400_000, 300_000)
        .unwrap();
    assert_eq!(first.accepted, vec![batch.intents[0].intent_id.clone()]);
    assert!(first.duplicates.is_empty());
    assert_eq!(
        store
            .due_push_deliveries(Timestamp(2_000), 32)
            .unwrap()
            .len(),
        1
    );

    let second = store
        .accept_notification_intents(&batch, Timestamp(2_100), 7 * 86_400_000, 300_000)
        .unwrap();
    assert!(second.accepted.is_empty());
    assert_eq!(second.duplicates, vec![batch.intents[0].intent_id.clone()]);
    assert_eq!(
        store
            .due_push_deliveries(Timestamp(2_100), 32)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn provider_mode_defaults_disabled_and_persists() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(
        store.push_provider_mode().unwrap(),
        PushProviderMode::Disabled
    );
    store
        .set_push_provider_mode(PushProviderMode::WebPush, Timestamp(1_000))
        .unwrap();
    assert_eq!(
        store.push_provider_mode().unwrap(),
        PushProviderMode::WebPush
    );
}
