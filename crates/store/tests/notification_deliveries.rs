//! Notification delivery persistence, deduplication, and queue bounds.

#![allow(clippy::unwrap_used)]

use protocol::{
    DurationMs, Event, EventId, EventKind, MachineId, NotificationCategory,
    NotificationDeliveryState, NotificationResultClass, SessionId, Timestamp,
};
use std::sync::atomic::{AtomicU64, Ordering};
use store::{NewNotificationDelivery, Store};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn event(id: u64, at: i64) -> Event {
    Event {
        id: EventId(id),
        session_id: SessionId(7),
        command_id: None,
        at: Timestamp(at),
        kind: EventKind::CommandFinished {
            exit_code: 0,
            duration_ms: DurationMs(10),
        },
    }
}

fn queued(event_id: u64, at: i64) -> NewNotificationDelivery {
    NewNotificationDelivery {
        event_id: EventId(event_id),
        machine_id: MachineId("machine-a".into()),
        session_id: SessionId(7),
        category: NotificationCategory::Completed,
        state: NotificationDeliveryState::Queued,
        at: Timestamp(at),
        deduplicate_since: None,
        pending_limit: 256,
    }
}

#[test]
fn migration_and_delivery_journal_survive_reopen() {
    let path = std::env::temp_dir().join(format!(
        "sentinal-notification-store-{}-{}.sqlite3",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 9);
        store.insert_event(&event(1, 1_000)).unwrap();
        let record = store
            .record_notification_delivery(&queued(1, 1_000))
            .unwrap();
        assert_eq!(record.state, NotificationDeliveryState::Queued);
        store
            .update_notification_delivery(
                record.id,
                NotificationDeliveryState::Delivered,
                1,
                None,
                Some(NotificationResultClass::Accepted),
                Some(187),
                Timestamp(1_001),
            )
            .unwrap();
    }
    let reopened = Store::open(&path).unwrap();
    let records = reopened.notification_deliveries(10).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, NotificationDeliveryState::Delivered);
    assert_eq!(
        records[0].result_class,
        Some(NotificationResultClass::Accepted)
    );
    assert_eq!(records[0].request_bytes, Some(187));
    assert_eq!(
        reopened.notification_event(EventId(1)).unwrap(),
        Some(event(1, 1_000))
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn deduplication_is_atomic_and_command_finished_can_bypass_it() {
    let store = Store::open_in_memory().unwrap();
    for id in 1..=3 {
        store.insert_event(&event(id, 1_000 + id as i64)).unwrap();
    }
    let first = store
        .record_notification_delivery(&queued(1, 1_001))
        .unwrap();
    assert_eq!(first.state, NotificationDeliveryState::Queued);

    let mut duplicate = queued(2, 1_002);
    duplicate.deduplicate_since = Some(Timestamp(500));
    let duplicate = store.record_notification_delivery(&duplicate).unwrap();
    assert_eq!(duplicate.state, NotificationDeliveryState::Deduplicated);

    let bypass = store
        .record_notification_delivery(&queued(3, 1_003))
        .unwrap();
    assert_eq!(bypass.state, NotificationDeliveryState::Queued);
}

#[test]
fn due_retry_and_pending_limit_are_bounded() {
    let store = Store::open_in_memory().unwrap();
    for id in 1..=3 {
        store.insert_event(&event(id, 1_000 + id as i64)).unwrap();
    }
    let mut first = queued(1, 1_001);
    first.pending_limit = 1;
    let first = store.record_notification_delivery(&first).unwrap();

    let mut overflow = queued(2, 1_002);
    overflow.pending_limit = 1;
    let overflow = store.record_notification_delivery(&overflow).unwrap();
    assert_eq!(overflow.state, NotificationDeliveryState::PermanentlyFailed);
    assert_eq!(
        overflow.result_class,
        Some(NotificationResultClass::QueueFull)
    );

    store
        .update_notification_delivery(
            first.id,
            NotificationDeliveryState::Retrying,
            1,
            Some(Timestamp(2_000)),
            Some(NotificationResultClass::ServerError),
            Some(120),
            Timestamp(1_500),
        )
        .unwrap();
    assert!(store
        .due_notification_deliveries(Timestamp(1_999), 10)
        .unwrap()
        .is_empty());
    let due = store
        .due_notification_deliveries(Timestamp(2_000), 10)
        .unwrap();
    assert_eq!(
        due.iter().map(|value| value.id).collect::<Vec<_>>(),
        vec![first.id]
    );

    let mut third = queued(3, 2_001);
    third.pending_limit = 1;
    let third = store.record_notification_delivery(&third).unwrap();
    assert_eq!(third.state, NotificationDeliveryState::PermanentlyFailed);
}

#[test]
fn shutdown_requeues_pending_and_retention_never_prunes_pending_work() {
    let store = Store::open_in_memory().unwrap();
    for id in 1..=2 {
        store.insert_event(&event(id, id as i64)).unwrap();
    }
    let delivered = store.record_notification_delivery(&queued(1, 1)).unwrap();
    store
        .update_notification_delivery(
            delivered.id,
            NotificationDeliveryState::Delivered,
            1,
            None,
            Some(NotificationResultClass::Accepted),
            Some(10),
            Timestamp(2),
        )
        .unwrap();
    let pending = store.record_notification_delivery(&queued(2, 1)).unwrap();

    assert_eq!(
        store.prune_notification_deliveries(Timestamp(10)).unwrap(),
        1
    );
    store
        .requeue_notification_deliveries_for_shutdown(Timestamp(20))
        .unwrap();
    let rows = store.notification_deliveries(10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, pending.id);
    assert_eq!(rows[0].state, NotificationDeliveryState::Queued);
    assert_eq!(rows[0].next_attempt_at, Some(Timestamp(20)));
    assert_eq!(
        rows[0].result_class,
        Some(NotificationResultClass::Shutdown)
    );
}

#[test]
fn worker_acknowledgement_advances_only_the_matching_forwarded_event() {
    let store = Store::open_in_memory().unwrap();
    for id in 1..=2 {
        store.insert_event(&event(id, id as i64)).unwrap();
        let mut delivery = queued(id, id as i64);
        delivery.state = NotificationDeliveryState::ForwardQueued;
        store.record_notification_delivery(&delivery).unwrap();
    }

    assert!(store
        .update_notification_delivery_for_event(
            EventId(1),
            NotificationDeliveryState::ControlAccepted,
            NotificationResultClass::ControlAccepted,
            Timestamp(50),
        )
        .unwrap());
    assert!(!store
        .update_notification_delivery_for_event(
            EventId(99),
            NotificationDeliveryState::ControlAccepted,
            NotificationResultClass::ControlAccepted,
            Timestamp(50),
        )
        .unwrap());

    let rows = store.notification_deliveries(10).unwrap();
    assert_eq!(rows[0].event_id, EventId(2));
    assert_eq!(rows[0].state, NotificationDeliveryState::ForwardQueued);
    assert_eq!(rows[1].event_id, EventId(1));
    assert_eq!(rows[1].state, NotificationDeliveryState::ControlAccepted);
    assert_eq!(
        rows[1].result_class,
        Some(NotificationResultClass::ControlAccepted)
    );
}
