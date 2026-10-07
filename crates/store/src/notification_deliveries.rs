//! Persistent, bounded notification decisions and retry state.

use crate::{to_sql_id, Store, StoreError};
use protocol::{
    Event, EventId, MachineId, NotificationCategory, NotificationDeliveryState,
    NotificationResultClass, SessionId, Timestamp,
};
use rusqlite::{params, OptionalExtension, Row};
use serde::{de::DeserializeOwned, Serialize};

/// Values needed to atomically record one notification decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotificationDelivery {
    /// Structured Event being considered.
    pub event_id: EventId,
    /// Machine that owns the Event.
    pub machine_id: MachineId,
    /// Session that owns the Event.
    pub session_id: SessionId,
    /// Stable Event-derived category.
    pub category: NotificationCategory,
    /// Initial decision state.
    pub state: NotificationDeliveryState,
    /// Decision timestamp.
    pub at: Timestamp,
    /// Deduplicate against decisions at or after this timestamp.
    pub deduplicate_since: Option<Timestamp>,
    /// Maximum queued/retrying rows.
    pub pending_limit: u32,
}

/// One persisted notification decision or delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDelivery {
    /// Local delivery journal ID.
    pub id: u64,
    /// Structured Event that caused it.
    pub event_id: EventId,
    /// Owning Machine.
    pub machine_id: MachineId,
    /// Owning Session.
    pub session_id: SessionId,
    /// Event-derived category.
    pub category: NotificationCategory,
    /// Current decision/delivery state.
    pub state: NotificationDeliveryState,
    /// External attempts already made.
    pub attempts: u32,
    /// Next attempt time for queued work.
    pub next_attempt_at: Option<Timestamp>,
    /// Bounded latest result.
    pub result_class: Option<NotificationResultClass>,
    /// Safe rendered request size.
    pub request_bytes: Option<u32>,
    /// First decision time.
    pub created_at: Timestamp,
    /// Last state-change time.
    pub updated_at: Timestamp,
}

impl Store {
    /// Atomically insert one decision, applying its deduplication and queue limit.
    pub fn record_notification_delivery(
        &self,
        value: &NewNotificationDelivery,
    ) -> Result<NotificationDelivery, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let category = enum_name(value.category)?;
        if let Some(existing) = transaction
            .query_row(
                "SELECT id, event_id, machine_id, session_id, category, state, attempts,
                        next_attempt_at, result_class, request_bytes, created_at, updated_at
                 FROM notification_deliveries WHERE event_id=?1 AND category=?2",
                params![to_sql_id(value.event_id.0)?, category],
                delivery_from_row,
            )
            .optional()?
        {
            return Ok(existing);
        }

        let mut state = value.state;
        let mut result_class = None;
        if let Some(since) = value.deduplicate_since {
            let duplicate: bool = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM notification_deliveries
                    WHERE machine_id=?1 AND session_id=?2 AND category=?3
                      AND created_at>=?4
                      AND state NOT IN ('RULE_DISABLED','NOT_CONFIGURED','DEDUPLICATED')
                )",
                params![
                    value.machine_id.0,
                    to_sql_id(value.session_id.0)?,
                    category,
                    since.0
                ],
                |row| row.get(0),
            )?;
            if duplicate {
                state = NotificationDeliveryState::Deduplicated;
            }
        }
        if state == NotificationDeliveryState::Queued {
            let pending: u32 = transaction.query_row(
                "SELECT COUNT(*) FROM notification_deliveries
                 WHERE state IN ('QUEUED','RETRYING')",
                [],
                |row| row.get(0),
            )?;
            if pending >= value.pending_limit {
                state = NotificationDeliveryState::PermanentlyFailed;
                result_class = Some(NotificationResultClass::QueueFull);
            }
        }
        let state_text = enum_name(state)?;
        let result_text = result_class.map(enum_name).transpose()?;
        let next_attempt_at = (state == NotificationDeliveryState::Queued).then_some(value.at.0);
        transaction.execute(
            "INSERT INTO notification_deliveries(
                event_id, machine_id, session_id, category, state, attempts,
                next_attempt_at, result_class, request_bytes, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,0,?6,?7,NULL,?8,?8)",
            params![
                to_sql_id(value.event_id.0)?,
                value.machine_id.0,
                to_sql_id(value.session_id.0)?,
                category,
                state_text,
                next_attempt_at,
                result_text,
                value.at.0,
            ],
        )?;
        let id = transaction.last_insert_rowid();
        let delivery = transaction.query_row(
            "SELECT id, event_id, machine_id, session_id, category, state, attempts,
                    next_attempt_at, result_class, request_bytes, created_at, updated_at
             FROM notification_deliveries WHERE id=?1",
            [id],
            delivery_from_row,
        )?;
        transaction.commit()?;
        Ok(delivery)
    }

    /// Update one queued delivery after an attempt or shutdown.
    #[allow(clippy::too_many_arguments)]
    pub fn update_notification_delivery(
        &self,
        id: u64,
        state: NotificationDeliveryState,
        attempts: u32,
        next_attempt_at: Option<Timestamp>,
        result_class: Option<NotificationResultClass>,
        request_bytes: Option<u32>,
        updated_at: Timestamp,
    ) -> Result<(), StoreError> {
        let changed = self.lock()?.execute(
            "UPDATE notification_deliveries
             SET state=?1, attempts=?2, next_attempt_at=?3, result_class=?4,
                 request_bytes=?5, updated_at=?6 WHERE id=?7",
            params![
                enum_name(state)?,
                attempts,
                next_attempt_at.map(|value| value.0),
                result_class.map(enum_name).transpose()?,
                request_bytes,
                updated_at.0,
                to_sql_id(id)?,
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::Missing {
                kind: "notification delivery",
                id,
            });
        }
        Ok(())
    }

    /// Update the local journal row for one Event after native control acknowledgement.
    pub fn update_notification_delivery_for_event(
        &self,
        event_id: EventId,
        state: NotificationDeliveryState,
        result_class: NotificationResultClass,
        updated_at: Timestamp,
    ) -> Result<bool, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE notification_deliveries
             SET state=?1,next_attempt_at=NULL,result_class=?2,updated_at=?3
             WHERE event_id=?4 AND state='FORWARD_QUEUED'",
            params![
                enum_name(state)?,
                enum_name(result_class)?,
                updated_at.0,
                to_sql_id(event_id.0)?,
            ],
        )? > 0)
    }

    /// Return due queued/retrying deliveries oldest first.
    pub fn due_notification_deliveries(
        &self,
        now: Timestamp,
        limit: u32,
    ) -> Result<Vec<NotificationDelivery>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, event_id, machine_id, session_id, category, state, attempts,
                    next_attempt_at, result_class, request_bytes, created_at, updated_at
             FROM notification_deliveries
             WHERE state IN ('QUEUED','RETRYING') AND next_attempt_at<=?1
             ORDER BY next_attempt_at, id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![now.0, limit], delivery_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Return newest delivery decisions first.
    pub fn notification_deliveries(
        &self,
        limit: u32,
    ) -> Result<Vec<NotificationDelivery>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, event_id, machine_id, session_id, category, state, attempts,
                    next_attempt_at, result_class, request_bytes, created_at, updated_at
             FROM notification_deliveries ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], delivery_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Delete a bounded batch of old terminal decisions while retaining pending work.
    pub fn prune_notification_deliveries(&self, before: Timestamp) -> Result<usize, StoreError> {
        Ok(self.lock()?.execute(
            "DELETE FROM notification_deliveries WHERE id IN (
                SELECT id FROM notification_deliveries
                WHERE updated_at<?1
                  AND state NOT IN ('QUEUED','RETRYING')
                ORDER BY id LIMIT 1000
             )",
            [before.0],
        )?)
    }

    /// Make interrupted notification attempts immediately retryable after restart.
    pub fn requeue_notification_deliveries_for_shutdown(
        &self,
        at: Timestamp,
    ) -> Result<usize, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE notification_deliveries
             SET state='QUEUED', next_attempt_at=?1, result_class='SHUTDOWN', updated_at=?1
             WHERE state IN ('QUEUED','RETRYING')",
            [at.0],
        )?)
    }

    /// Load the structured Event referenced by a delivery row.
    pub fn notification_event(&self, event_id: EventId) -> Result<Option<Event>, StoreError> {
        Ok(self
            .events(None, Some(EventId(event_id.0.saturating_sub(1))), 1)?
            .into_iter()
            .find(|event| event.id == event_id))
    }
}

fn delivery_from_row(row: &Row<'_>) -> rusqlite::Result<NotificationDelivery> {
    Ok(NotificationDelivery {
        id: from_sql_id(row.get(0)?)?,
        event_id: EventId(from_sql_id(row.get(1)?)?),
        machine_id: MachineId(row.get(2)?),
        session_id: SessionId(from_sql_id(row.get(3)?)?),
        category: parse_enum(row.get(4)?)?,
        state: parse_enum(row.get(5)?)?,
        attempts: row.get(6)?,
        next_attempt_at: row.get::<_, Option<i64>>(7)?.map(Timestamp),
        result_class: row
            .get::<_, Option<String>>(8)?
            .map(parse_enum)
            .transpose()?,
        request_bytes: row.get(9)?,
        created_at: Timestamp(row.get(10)?),
        updated_at: Timestamp(row.get(11)?),
    })
}

fn enum_name(value: impl Serialize) -> Result<String, StoreError> {
    Ok(serde_json::to_string(&value)?.trim_matches('"').to_string())
}

fn parse_enum<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&format!("\"{value}\"")).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn from_sql_id(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
