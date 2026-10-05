//! Background Event ingestion, delivery, and configuration reload tasks.

use super::NotificationService;
use protocol::Event;
use std::{sync::Arc, time::Duration};
use store::Store;

/// Record every Session Event without blocking its producer.
pub fn spawn_notifier(
    service: NotificationService,
    mut events: tokio::sync::broadcast::Receiver<Event>,
    store: Arc<Store>,
    _machine: String,
) {
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let Some(session) = store.session(event.session_id).ok().flatten() else {
                        continue;
                    };
                    if let Err(error) = service.enqueue(&event, session.capture) {
                        tracing::warn!(%error, "failed to enqueue notification Event");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "notification Event stream lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Start bounded delivery and configuration-reload timers.
pub fn spawn_notification_workers(
    service: NotificationService,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let delivery = service.clone();
    let mut delivery_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        'delivery: loop {
            tokio::select! {
                _ = delivery_shutdown.changed() => break,
                _ = interval.tick() => {
                    tokio::select! {
                        _ = delivery_shutdown.changed() => break 'delivery,
                        result = delivery.deliver_due_once() => {
                            if let Err(error) = result {
                                tracing::warn!(%error, "notification delivery scan failed");
                            }
                        }
                    }
                }
            }
        }
        if let Err(error) = delivery
            .store
            .requeue_notification_deliveries_for_shutdown((delivery.clock)())
        {
            tracing::warn!(%error, "failed to requeue notification delivery during shutdown");
        }
    });
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.tick().await;
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = interval.tick() => {
                    service.reload_config();
                }
            }
        }
    });
}
