//! Durable worker intents, control subscriptions, and Web Push delivery rows.

use crate::{to_sql_id, Store, StoreError};
use protocol::{
    MachineId, NotificationCategory, NotificationDeliveryState, NotificationFacts,
    NotificationIntent, NotificationIntentAcknowledgement, NotificationIntentBatch,
    NotificationIntentRejection, NotificationIntentRejectionCode, NotificationResultClass,
    PushProviderMode, PushSubscriptionInfo, PushSubscriptionKeys, PushSubscriptionRequest,
    PushSubscriptionState, SessionId, Timestamp,
};
use rusqlite::{params, OptionalExtension, Row, Transaction};
use serde::{de::DeserializeOwned, Serialize};

const CONTROL_DEDUP_MS: i64 = 5 * 60 * 1_000;

/// Values required to enqueue one worker notification intent.
#[derive(Debug, Clone, PartialEq)]
pub struct NewNotificationIntentOutbox {
    /// Worker Machine that owns the Event.
    pub machine_id: MachineId,
    /// Privacy-safe Event-derived intent.
    pub intent: NotificationIntent,
    /// Enqueue timestamp.
    pub at: Timestamp,
    /// Maximum pending rows.
    pub pending_limit: u32,
    /// Maximum encoded bytes across pending rows.
    pub pending_bytes_limit: u64,
}

/// Result of idempotently enqueuing one worker intent.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationIntentOutboxInsert {
    /// Whether this call inserted a new row.
    pub inserted: bool,
    /// Stored row.
    pub row: StoredNotificationIntent,
}

/// One persisted worker intent waiting for control acknowledgement.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredNotificationIntent {
    /// Owning Machine.
    pub machine_id: MachineId,
    /// Safe intent payload.
    pub intent: NotificationIntent,
    /// Upload attempts already made.
    pub attempts: u32,
    /// Next due upload time.
    pub next_attempt_at: Timestamp,
    /// Last bounded upload result.
    pub last_result: Option<String>,
    /// Enqueue timestamp.
    pub created_at: Timestamp,
    /// Last state change.
    pub updated_at: Timestamp,
}

/// One stored Push subscription including provider secrets for delivery only.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredPushSubscription {
    /// Secret-free owner-visible metadata.
    pub info: PushSubscriptionInfo,
    /// SHA-256 endpoint uniqueness key.
    pub endpoint_hash: String,
    /// Browser-managed opaque endpoint; never expose to UI/logs.
    pub endpoint: String,
    /// Browser-managed encryption material; Debug is redacted.
    pub keys: PushSubscriptionKeys,
}

impl std::fmt::Debug for StoredPushSubscription {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredPushSubscription")
            .field("info", &self.info)
            .field("endpoint_hash", &"[redacted]")
            .field("endpoint", &"[redacted]")
            .field("keys", &self.keys)
            .finish()
    }
}

/// One due control delivery with all data needed by an injected provider.
#[derive(Debug, Clone, PartialEq)]
pub struct PushDelivery {
    /// Delivery row ID.
    pub id: u64,
    /// Worker Machine that emitted the intent.
    pub machine_id: MachineId,
    /// Worker Session that emitted the intent.
    pub session_id: SessionId,
    /// Privacy-safe intent.
    pub intent: NotificationIntent,
    /// Derived category.
    pub category: NotificationCategory,
    /// Target browser subscription.
    pub subscription: StoredPushSubscription,
    /// Current delivery state.
    pub state: NotificationDeliveryState,
    /// Attempts already made.
    pub attempts: u32,
    /// Next scheduled attempt.
    pub next_attempt_at: Option<Timestamp>,
    /// Latest bounded provider result.
    pub result_class: Option<NotificationResultClass>,
    /// Encrypted provider request size when attempted.
    pub request_bytes: Option<u32>,
    /// Last delivery state change.
    pub updated_at: Timestamp,
}

impl Store {
    /// Idempotently enqueue one privacy-safe worker intent within row/byte bounds.
    pub fn record_notification_intent_outbox(
        &self,
        value: &NewNotificationIntentOutbox,
    ) -> Result<NotificationIntentOutboxInsert, StoreError> {
        value.intent.validate()?;
        let facts_json = serde_json::to_string(&value.intent.facts)?;
        let encoded_bytes = serde_json::to_vec(&value.intent)?.len() as u64;
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        if let Some(row) = transaction
            .query_row(
                "SELECT machine_id,intent_id,event_id,session_id,occurred_at,session_label,
                        facts_json,attempts,next_attempt_at,last_result,created_at,updated_at
                 FROM notification_intent_outbox WHERE intent_id=?1",
                [&value.intent.intent_id],
                outbox_from_row,
            )
            .optional()?
        {
            return Ok(NotificationIntentOutboxInsert {
                inserted: false,
                row,
            });
        }
        let (rows, bytes): (u32, u64) = transaction.query_row(
            "SELECT COUNT(*), COALESCE(SUM(encoded_bytes),0) FROM notification_intent_outbox",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if rows >= value.pending_limit
            || bytes.saturating_add(encoded_bytes) > value.pending_bytes_limit
        {
            return Err(StoreError::PushOutboxFull {
                rows,
                bytes,
                maximum_rows: value.pending_limit,
                maximum_bytes: value.pending_bytes_limit,
            });
        }
        transaction.execute(
            "INSERT INTO notification_intent_outbox(
                intent_id,machine_id,event_id,session_id,occurred_at,session_label,facts_json,
                encoded_bytes,attempts,next_attempt_at,last_result,created_at,updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0,?9,NULL,?9,?9)",
            params![
                value.intent.intent_id,
                value.machine_id.0,
                to_sql_id(value.intent.event_id.0)?,
                to_sql_id(value.intent.session_id.0)?,
                value.intent.occurred_at.0,
                value.intent.session_label,
                facts_json,
                encoded_bytes,
                value.at.0,
            ],
        )?;
        let row = transaction.query_row(
            "SELECT machine_id,intent_id,event_id,session_id,occurred_at,session_label,
                    facts_json,attempts,next_attempt_at,last_result,created_at,updated_at
             FROM notification_intent_outbox WHERE intent_id=?1",
            [&value.intent.intent_id],
            outbox_from_row,
        )?;
        transaction.commit()?;
        Ok(NotificationIntentOutboxInsert {
            inserted: true,
            row,
        })
    }

    /// Return currently due worker intents in deterministic Event order.
    pub fn due_notification_intents(
        &self,
        now: Timestamp,
        limit: u32,
    ) -> Result<Vec<StoredNotificationIntent>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT machine_id,intent_id,event_id,session_id,occurred_at,session_label,
                    facts_json,attempts,next_attempt_at,last_result,created_at,updated_at
             FROM notification_intent_outbox WHERE next_attempt_at<=?1
             ORDER BY occurred_at,event_id,intent_id LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![now.0, limit], outbox_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Delete only intents durably acknowledged by the control.
    pub fn acknowledge_notification_intents(
        &self,
        intent_ids: &[String],
    ) -> Result<usize, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let mut removed = 0;
        for intent_id in intent_ids {
            removed += transaction.execute(
                "DELETE FROM notification_intent_outbox WHERE intent_id=?1",
                [intent_id],
            )?;
        }
        transaction.commit()?;
        Ok(removed)
    }

    /// Reschedule one worker intent after a bounded upload result.
    pub fn reschedule_notification_intent(
        &self,
        intent_id: &str,
        attempts: u32,
        next_attempt_at: Timestamp,
        last_result: &str,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        let changed = self.lock()?.execute(
            "UPDATE notification_intent_outbox SET attempts=?1,next_attempt_at=?2,
                    last_result=?3,updated_at=?4 WHERE intent_id=?5",
            params![attempts, next_attempt_at.0, last_result, at.0, intent_id],
        )?;
        if changed == 0 {
            return Err(StoreError::MissingText {
                kind: "notification intent",
                id: intent_id.into(),
            });
        }
        Ok(())
    }

    /// Insert or refresh a browser subscription, unique by endpoint hash.
    pub fn upsert_push_subscription(
        &self,
        proposed_id: &str,
        endpoint_hash: &str,
        request: &PushSubscriptionRequest,
        at: Timestamp,
        maximum_devices: u32,
    ) -> Result<StoredPushSubscription, StoreError> {
        request.validate()?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let existing_id: Option<String> = transaction
            .query_row(
                "SELECT subscription_id FROM push_subscriptions WHERE endpoint_hash=?1",
                [endpoint_hash],
                |row| row.get(0),
            )
            .optional()?;
        let subscription_id = if let Some(id) = existing_id {
            id
        } else {
            let active: u32 = transaction.query_row(
                "SELECT COUNT(*) FROM push_subscriptions WHERE state='ACTIVE'",
                [],
                |row| row.get(0),
            )?;
            if active >= maximum_devices {
                return Err(StoreError::PushDeviceLimit {
                    maximum: maximum_devices,
                });
            }
            proposed_id.to_string()
        };
        transaction.execute(
            "INSERT INTO push_subscriptions(
                subscription_id,endpoint_hash,endpoint,p256dh,auth,device_label,state,
                created_at,updated_at,last_success_at,expiration_time
             ) VALUES (?1,?2,?3,?4,?5,?6,'ACTIVE',?7,?7,NULL,?8)
             ON CONFLICT(endpoint_hash) DO UPDATE SET
                endpoint=excluded.endpoint,p256dh=excluded.p256dh,auth=excluded.auth,
                device_label=excluded.device_label,state='ACTIVE',updated_at=excluded.updated_at,
                expiration_time=excluded.expiration_time",
            params![
                subscription_id,
                endpoint_hash,
                request.endpoint,
                request.keys.p256dh,
                request.keys.auth,
                request.device_label,
                at.0,
                request.expiration_time.map(|value| value.0),
            ],
        )?;
        let stored = subscription_by_id(&transaction, &subscription_id)?.ok_or_else(|| {
            StoreError::MissingText {
                kind: "push subscription",
                id: subscription_id.clone(),
            }
        })?;
        transaction.commit()?;
        Ok(stored)
    }

    /// Return all stored subscriptions including secrets for control internals.
    pub fn push_subscriptions(&self) -> Result<Vec<StoredPushSubscription>, StoreError> {
        let connection = self.lock()?;
        subscriptions_query(&connection, None)
    }

    /// Return enabled subscriptions including secrets for provider fan-out.
    pub fn active_push_subscriptions(&self) -> Result<Vec<StoredPushSubscription>, StoreError> {
        let connection = self.lock()?;
        subscriptions_query(&connection, Some(PushSubscriptionState::Active))
    }

    /// Return only owner-visible subscription metadata.
    pub fn push_subscription_infos(&self) -> Result<Vec<PushSubscriptionInfo>, StoreError> {
        Ok(self
            .push_subscriptions()?
            .into_iter()
            .map(|value| value.info)
            .collect())
    }

    /// Load one subscription including provider secrets for internal delivery.
    pub fn push_subscription_secret(
        &self,
        subscription_id: &str,
    ) -> Result<Option<StoredPushSubscription>, StoreError> {
        let connection = self.lock()?;
        subscription_by_id(&connection, subscription_id)
    }

    /// Set one subscription state without affecting other devices.
    pub fn set_push_subscription_state(
        &self,
        subscription_id: &str,
        state: PushSubscriptionState,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        let changed = self.lock()?.execute(
            "UPDATE push_subscriptions SET state=?1,updated_at=?2 WHERE subscription_id=?3",
            params![enum_name(state)?, at.0, subscription_id],
        )?;
        if changed == 0 {
            return Err(StoreError::MissingText {
                kind: "push subscription",
                id: subscription_id.into(),
            });
        }
        Ok(())
    }

    /// Permanently remove one owner-selected browser subscription.
    pub fn remove_push_subscription(&self, subscription_id: &str) -> Result<bool, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM push_deliveries WHERE subscription_id=?1",
            [subscription_id],
        )?;
        let removed = transaction.execute(
            "DELETE FROM push_subscriptions WHERE subscription_id=?1",
            [subscription_id],
        )?;
        transaction.commit()?;
        Ok(removed > 0)
    }

    /// Read the mutually exclusive external provider mode.
    pub fn push_provider_mode(&self) -> Result<PushProviderMode, StoreError> {
        let value: String = self.lock()?.query_row(
            "SELECT mode FROM notification_provider_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        parse_enum(value).map_err(StoreError::Sql)
    }

    /// Persist the owner-selected provider mode.
    pub fn set_push_provider_mode(
        &self,
        mode: PushProviderMode,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.lock()?.execute(
            "UPDATE notification_provider_state SET mode=?1,updated_at=?2 WHERE singleton=1",
            params![enum_name(mode)?, at.0],
        )?;
        Ok(())
    }

    /// Atomically accept/deduplicate a worker batch and fan out to active devices.
    pub fn accept_notification_intents(
        &self,
        batch: &NotificationIntentBatch,
        now: Timestamp,
        maximum_age_ms: i64,
        maximum_future_skew_ms: i64,
    ) -> Result<NotificationIntentAcknowledgement, StoreError> {
        batch.validate()?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let mut acknowledgement = NotificationIntentAcknowledgement {
            accepted: Vec::new(),
            duplicates: Vec::new(),
            rejected: Vec::new(),
        };
        for intent in &batch.intents {
            if intent.occurred_at.0 < now.0.saturating_sub(maximum_age_ms) {
                acknowledgement.rejected.push(NotificationIntentRejection {
                    intent_id: intent.intent_id.clone(),
                    code: NotificationIntentRejectionCode::TooOld,
                });
                continue;
            }
            if intent.occurred_at.0 > now.0.saturating_add(maximum_future_skew_ms) {
                acknowledgement.rejected.push(NotificationIntentRejection {
                    intent_id: intent.intent_id.clone(),
                    code: NotificationIntentRejectionCode::ClockSkew,
                });
                continue;
            }
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM control_notification_intents
                               WHERE machine_id=?1 AND intent_id=?2)",
                params![batch.machine_id.0, intent.intent_id],
                |row| row.get(0),
            )?;
            if exists {
                acknowledgement.duplicates.push(intent.intent_id.clone());
                continue;
            }
            accept_one_intent(&transaction, batch, intent, now)?;
            acknowledgement.accepted.push(intent.intent_id.clone());
        }
        transaction.commit()?;
        Ok(acknowledgement)
    }

    /// Return due provider deliveries with subscription secrets for the sender only.
    pub fn due_push_deliveries(
        &self,
        now: Timestamp,
        limit: u32,
    ) -> Result<Vec<PushDelivery>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT d.id,d.machine_id,d.session_id,d.category,d.state,d.attempts,
                    d.next_attempt_at,d.result_class,d.request_bytes,d.updated_at,
                    i.intent_id,i.event_id,i.occurred_at,i.session_label,i.facts_json,
                    s.subscription_id,s.endpoint_hash,s.endpoint,s.p256dh,s.auth,s.device_label,
                    s.state,s.created_at,s.updated_at,s.last_success_at,s.expiration_time
             FROM push_deliveries d
             JOIN control_notification_intents i ON i.id=d.control_intent_id
             JOIN push_subscriptions s ON s.subscription_id=d.subscription_id
             WHERE d.state IN ('QUEUED','RETRYING') AND d.next_attempt_at<=?1
               AND s.state='ACTIVE'
             ORDER BY d.next_attempt_at,d.id LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![now.0, limit], push_delivery_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Return newest control provider deliveries for secret-free owner history.
    pub fn push_delivery_history(&self, limit: u32) -> Result<Vec<PushDelivery>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT d.id,d.machine_id,d.session_id,d.category,d.state,d.attempts,
                    d.next_attempt_at,d.result_class,d.request_bytes,d.updated_at,
                    i.intent_id,i.event_id,i.occurred_at,i.session_label,i.facts_json,
                    s.subscription_id,s.endpoint_hash,s.endpoint,s.p256dh,s.auth,s.device_label,
                    s.state,s.created_at,s.updated_at,s.last_success_at,s.expiration_time
             FROM push_deliveries d
             JOIN control_notification_intents i ON i.id=d.control_intent_id
             JOIN push_subscriptions s ON s.subscription_id=d.subscription_id
             ORDER BY d.id DESC LIMIT ?1",
        )?;
        let rows = statement
            .query_map([limit.min(1_000)], push_delivery_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Update one provider delivery and optionally its device success timestamp.
    #[allow(clippy::too_many_arguments)]
    pub fn update_push_delivery(
        &self,
        id: u64,
        state: NotificationDeliveryState,
        attempts: u32,
        next_attempt_at: Option<Timestamp>,
        result_class: Option<NotificationResultClass>,
        request_bytes: Option<u32>,
        subscription_id: &str,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE push_deliveries SET state=?1,attempts=?2,next_attempt_at=?3,
                    result_class=?4,request_bytes=?5,updated_at=?6 WHERE id=?7",
            params![
                enum_name(state)?,
                attempts,
                next_attempt_at.map(|value| value.0),
                result_class.map(enum_name).transpose()?,
                request_bytes,
                at.0,
                to_sql_id(id)?,
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::Missing {
                kind: "push delivery",
                id,
            });
        }
        if state == NotificationDeliveryState::Delivered {
            transaction.execute(
                "UPDATE push_subscriptions SET last_success_at=?1,updated_at=?1
                 WHERE subscription_id=?2",
                params![at.0, subscription_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn accept_one_intent(
    transaction: &Transaction<'_>,
    batch: &NotificationIntentBatch,
    intent: &NotificationIntent,
    now: Timestamp,
) -> Result<(), StoreError> {
    let category = intent.facts.category();
    let category_text = enum_name(category)?;
    let bypass_dedup = matches!(
        category,
        NotificationCategory::Completed | NotificationCategory::Failed
    );
    let duplicate: bool = if bypass_dedup {
        false
    } else {
        transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM control_notification_intents
              WHERE machine_id=?1 AND session_id=?2 AND category=?3
                AND created_at>=?4 AND decision='ACCEPTED')",
            params![
                batch.machine_id.0,
                to_sql_id(intent.session_id.0)?,
                category_text,
                now.0.saturating_sub(CONTROL_DEDUP_MS),
            ],
            |row| row.get(0),
        )?
    };
    let decision = if duplicate {
        "DEDUPLICATED"
    } else {
        "ACCEPTED"
    };
    transaction.execute(
        "INSERT INTO control_notification_intents(
            machine_id,intent_id,event_id,session_id,occurred_at,session_label,facts_json,
            category,decision,created_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            batch.machine_id.0,
            intent.intent_id,
            to_sql_id(intent.event_id.0)?,
            to_sql_id(intent.session_id.0)?,
            intent.occurred_at.0,
            intent.session_label,
            serde_json::to_string(&intent.facts)?,
            category_text,
            decision,
            now.0,
        ],
    )?;
    if duplicate {
        return Ok(());
    }
    let control_intent_id = transaction.last_insert_rowid();
    let mut statement = transaction.prepare(
        "SELECT subscription_id FROM push_subscriptions WHERE state='ACTIVE'
         ORDER BY subscription_id LIMIT 16",
    )?;
    let subscriptions = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for subscription_id in subscriptions {
        transaction.execute(
            "INSERT INTO push_deliveries(
                control_intent_id,machine_id,session_id,subscription_id,category,state,
                attempts,next_attempt_at,result_class,request_bytes,created_at,updated_at
             ) VALUES (?1,?2,?3,?4,?5,'QUEUED',0,?6,NULL,NULL,?6,?6)",
            params![
                control_intent_id,
                batch.machine_id.0,
                to_sql_id(intent.session_id.0)?,
                subscription_id,
                category_text,
                now.0,
            ],
        )?;
    }
    Ok(())
}

fn outbox_from_row(row: &Row<'_>) -> rusqlite::Result<StoredNotificationIntent> {
    let facts_json: String = row.get(6)?;
    Ok(StoredNotificationIntent {
        machine_id: MachineId(row.get(0)?),
        intent: NotificationIntent {
            intent_id: row.get(1)?,
            event_id: protocol::EventId(from_sql_id(row.get(2)?)?),
            session_id: SessionId(from_sql_id(row.get(3)?)?),
            occurred_at: Timestamp(row.get(4)?),
            session_label: row.get(5)?,
            facts: serde_json::from_str(&facts_json).map_err(json_from_sql)?,
        },
        attempts: row.get(7)?,
        next_attempt_at: Timestamp(row.get(8)?),
        last_result: row.get(9)?,
        created_at: Timestamp(row.get(10)?),
        updated_at: Timestamp(row.get(11)?),
    })
}

fn subscriptions_query(
    connection: &rusqlite::Connection,
    state: Option<PushSubscriptionState>,
) -> Result<Vec<StoredPushSubscription>, StoreError> {
    let sql = "SELECT subscription_id,endpoint_hash,endpoint,p256dh,auth,device_label,state,
                      created_at,updated_at,last_success_at,expiration_time
               FROM push_subscriptions WHERE (?1 IS NULL OR state=?1)
               ORDER BY created_at,subscription_id";
    let state_text = state.map(enum_name).transpose()?;
    let mut statement = connection.prepare(sql)?;
    let rows = statement
        .query_map([state_text], subscription_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn subscription_by_id(
    connection: &rusqlite::Connection,
    subscription_id: &str,
) -> Result<Option<StoredPushSubscription>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT subscription_id,endpoint_hash,endpoint,p256dh,auth,device_label,state,
                    created_at,updated_at,last_success_at,expiration_time
             FROM push_subscriptions WHERE subscription_id=?1",
            [subscription_id],
            subscription_from_row,
        )
        .optional()?)
}

fn subscription_from_row(row: &Row<'_>) -> rusqlite::Result<StoredPushSubscription> {
    Ok(StoredPushSubscription {
        info: PushSubscriptionInfo {
            subscription_id: row.get(0)?,
            device_label: row.get(5)?,
            state: parse_enum(row.get(6)?)?,
            created_at: Timestamp(row.get(7)?),
            updated_at: Timestamp(row.get(8)?),
            last_success_at: row.get::<_, Option<i64>>(9)?.map(Timestamp),
            expiration_time: row.get::<_, Option<i64>>(10)?.map(Timestamp),
        },
        endpoint_hash: row.get(1)?,
        endpoint: row.get(2)?,
        keys: PushSubscriptionKeys {
            p256dh: row.get(3)?,
            auth: row.get(4)?,
        },
    })
}

fn push_delivery_from_row(row: &Row<'_>) -> rusqlite::Result<PushDelivery> {
    let facts: NotificationFacts =
        serde_json::from_str(&row.get::<_, String>(14)?).map_err(json_from_sql)?;
    Ok(PushDelivery {
        id: from_sql_id(row.get(0)?)?,
        machine_id: MachineId(row.get(1)?),
        session_id: SessionId(from_sql_id(row.get(2)?)?),
        category: parse_enum(row.get(3)?)?,
        state: parse_enum(row.get(4)?)?,
        attempts: row.get(5)?,
        next_attempt_at: row.get::<_, Option<i64>>(6)?.map(Timestamp),
        result_class: row
            .get::<_, Option<String>>(7)?
            .map(parse_enum)
            .transpose()?,
        intent: NotificationIntent {
            intent_id: row.get(10)?,
            event_id: protocol::EventId(from_sql_id(row.get(11)?)?),
            occurred_at: Timestamp(row.get(12)?),
            session_label: row.get(13)?,
            session_id: SessionId(from_sql_id(row.get(2)?)?),
            facts,
        },
        subscription: StoredPushSubscription {
            info: PushSubscriptionInfo {
                subscription_id: row.get(15)?,
                device_label: row.get(20)?,
                state: parse_enum(row.get(21)?)?,
                created_at: Timestamp(row.get(22)?),
                updated_at: Timestamp(row.get(23)?),
                last_success_at: row.get::<_, Option<i64>>(24)?.map(Timestamp),
                expiration_time: row.get::<_, Option<i64>>(25)?.map(Timestamp),
            },
            endpoint_hash: row.get(16)?,
            endpoint: row.get(17)?,
            keys: PushSubscriptionKeys {
                p256dh: row.get(18)?,
                auth: row.get(19)?,
            },
        },
        request_bytes: row.get(8)?,
        updated_at: Timestamp(row.get(9)?),
    })
}

fn enum_name(value: impl Serialize) -> Result<String, StoreError> {
    Ok(serde_json::to_string(&value)?.trim_matches('"').to_string())
}

fn parse_enum<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&format!("\"{value}\"")).map_err(json_from_sql)
}

fn json_from_sql(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
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
