//! Bounded, DNS-pinned Web Push transport and retry classification.

use super::{
    endpoint::validate_resolved_addresses,
    provider::{BuiltPushRequest, PushPriority, SafePushPayload, WebPushProvider},
};
use futures_util::{stream, StreamExt};
use protocol::{
    NotificationDeliveryState, NotificationFacts, NotificationJournalEntry,
    NotificationResultClass, Resource, Timestamp,
};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    time::Duration,
};
use store::PushDelivery;

use super::gateway::{PushGateway, PushGatewayError};

const DELIVERY_BATCH: u32 = 32;
const DELIVERY_CONCURRENCY: usize = 4;
const RETRY_MS: [i64; 4] = [5_000, 30_000, 120_000, 300_000];

/// Secret-free provider outcome used by the retry scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushSendResult {
    /// Provider accepted the encrypted request.
    Accepted,
    /// Request timed out.
    Timeout,
    /// DNS, TLS, or connection failed.
    ConnectionFailed,
    /// Subscription expired (`404` or `410`).
    Expired,
    /// Encrypted request exceeded provider bounds (`413`).
    PayloadTooLarge,
    /// Provider rate limited the request.
    RateLimited(Option<u64>),
    /// Other permanent `4xx` response.
    ClientError,
    /// Retryable `5xx`, `408`, or `425` response.
    ServerError,
}

/// Boxed provider future used by real and fake transports.
pub type PushSendFuture = Pin<Box<dyn Future<Output = PushSendResult> + Send>>;

/// One Web Push transport implementation; fake transports exercise scheduling offline.
pub trait PushTransport: Send + Sync {
    /// Send one encrypted request to already revalidated/pinned addresses.
    fn send(&self, request: BuiltPushRequest, addresses: Vec<IpAddr>) -> PushSendFuture;
}

/// Production HTTPS transport with redirects disabled and DNS pinned per attempt.
pub struct ReqwestPushTransport;

impl PushTransport for ReqwestPushTransport {
    fn send(&self, request: BuiltPushRequest, addresses: Vec<IpAddr>) -> PushSendFuture {
        Box::pin(async move {
            let host = request.host().to_string();
            let sockets: Vec<_> = addresses
                .into_iter()
                .map(|address| SocketAddr::new(address, 443))
                .collect();
            let client = match reqwest::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .resolve_to_addrs(&host, &sockets)
                .build()
            {
                Ok(client) => client,
                Err(_) => return PushSendResult::ConnectionFailed,
            };
            let (url, headers, body) = request.into_parts();
            let mut builder = client.post(url).body(body);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            match builder.send().await {
                Ok(response) if response.status().is_success() => PushSendResult::Accepted,
                Ok(response) if matches!(response.status().as_u16(), 404 | 410) => {
                    PushSendResult::Expired
                }
                Ok(response) if response.status().as_u16() == 413 => {
                    PushSendResult::PayloadTooLarge
                }
                Ok(response) if response.status().as_u16() == 429 => {
                    let seconds = response
                        .headers()
                        .get("retry-after")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse().ok());
                    PushSendResult::RateLimited(seconds)
                }
                Ok(response)
                    if matches!(response.status().as_u16(), 408 | 425)
                        || response.status().is_server_error() =>
                {
                    PushSendResult::ServerError
                }
                Ok(_) => PushSendResult::ClientError,
                Err(error) if error.is_timeout() => PushSendResult::Timeout,
                Err(_) => PushSendResult::ConnectionFailed,
            }
        })
    }
}

impl PushGateway {
    /// Return bounded, secret-free provider delivery history for the owner UI.
    pub fn delivery_history(&self) -> Result<Vec<NotificationJournalEntry>, PushGatewayError> {
        let deliveries = self
            .store()
            .push_delivery_history(1_000)
            .map_err(|_| PushGatewayError::Internal)?;
        deliveries
            .into_iter()
            .map(|delivery| {
                let machine_name = self
                    .store()
                    .registry_machine(&delivery.machine_id)
                    .map_err(|_| PushGatewayError::Internal)?
                    .map_or_else(|| "Machine".to_string(), |machine| machine.display_name);
                let payload = payload_for(&delivery, &machine_name)
                    .map_err(|_| PushGatewayError::Internal)?;
                Ok(NotificationJournalEntry {
                    id: delivery.id,
                    machine_id: delivery.machine_id,
                    session_id: delivery.session_id,
                    text: format!("{}: {}", payload.title, payload.body),
                    at: delivery.updated_at,
                    delivered: delivery.state == NotificationDeliveryState::Delivered,
                    event_id: Some(delivery.intent.event_id),
                    category: Some(delivery.category),
                    state: Some(delivery.state),
                    attempts: Some(delivery.attempts),
                    result_class: delivery.result_class,
                    request_bytes: delivery.request_bytes,
                })
            })
            .collect()
    }

    /// Attempt currently due provider deliveries with bounded concurrency.
    pub async fn deliver_due_once(&self) -> Result<usize, PushGatewayError> {
        let due = self
            .store()
            .due_push_deliveries(self.now(), DELIVERY_BATCH)
            .map_err(|_| PushGatewayError::Internal)?;
        let results = stream::iter(due.into_iter().map(|delivery| {
            let gateway = self.clone();
            async move { gateway.deliver_one(delivery).await }
        }))
        .buffer_unordered(DELIVERY_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        let mut delivered = 0;
        for result in results {
            if result? {
                delivered += 1;
            }
        }
        Ok(delivered)
    }

    async fn deliver_one(&self, delivery: PushDelivery) -> Result<bool, PushGatewayError> {
        if delivery
            .subscription
            .info
            .expiration_time
            .is_some_and(|expiration| expiration.0 <= self.now().0)
        {
            return self.finish_delivery(delivery, PushSendResult::Expired, None);
        }
        let shape =
            match super::endpoint::validate_push_endpoint_shape(&delivery.subscription.endpoint) {
                Ok(shape) => shape,
                Err(_) => return self.finish_delivery(delivery, PushSendResult::ClientError, None),
            };
        let addresses = match (self.resolver())(shape.host, shape.port).await {
            Ok(addresses) => addresses,
            Err(_) => {
                return self.finish_delivery(delivery, PushSendResult::ConnectionFailed, None)
            }
        };
        if validate_resolved_addresses(&addresses).is_err() {
            return self.finish_delivery(delivery, PushSendResult::ClientError, None);
        }
        let machine_name = self
            .store()
            .registry_machine(&delivery.machine_id)
            .ok()
            .flatten()
            .map_or_else(|| "Machine".to_string(), |machine| machine.display_name);
        let payload = match payload_for(&delivery, &machine_name) {
            Ok(payload) => payload,
            Err(_) => return self.finish_delivery(delivery, PushSendResult::PayloadTooLarge, None),
        };
        let key = self.signing_key()?;
        let request = match WebPushProvider::build_request(
            &delivery.subscription.endpoint,
            &delivery.subscription.keys.p256dh,
            &delivery.subscription.keys.auth,
            &payload,
            &key,
        ) {
            Ok(request) => request,
            Err(_) => return self.finish_delivery(delivery, PushSendResult::ClientError, None),
        };
        let request_bytes = Some(request.encoded_size());
        let result = self.transport().send(request, addresses).await;
        self.finish_delivery(delivery, result, request_bytes)
    }

    fn finish_delivery(
        &self,
        delivery: PushDelivery,
        result: PushSendResult,
        request_bytes: Option<u32>,
    ) -> Result<bool, PushGatewayError> {
        let attempts = delivery.attempts.saturating_add(1);
        let now = self.now();
        let (state, next, class) = classify(result, attempts, now);
        self.store()
            .update_push_delivery(
                delivery.id,
                state,
                attempts,
                next,
                Some(class),
                request_bytes,
                &delivery.subscription.info.subscription_id,
                now,
            )
            .map_err(|_| PushGatewayError::Internal)?;
        if result == PushSendResult::Expired {
            self.store()
                .set_push_subscription_state(
                    &delivery.subscription.info.subscription_id,
                    protocol::PushSubscriptionState::Expired,
                    now,
                )
                .map_err(|_| PushGatewayError::Internal)?;
        }
        Ok(state == NotificationDeliveryState::Delivered)
    }
}

/// Start the control-owned Web Push delivery scheduler.
pub fn spawn_push_delivery_worker(
    gateway: PushGateway,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = interval.tick() => {
                    if gateway.notification_ingress() {
                        if let Err(error) = gateway.deliver_due_once().await {
                            tracing::warn!(%error, "Web Push delivery scan failed");
                        }
                    }
                }
            }
        }
    });
}

fn classify(
    result: PushSendResult,
    attempts: u32,
    now: Timestamp,
) -> (
    NotificationDeliveryState,
    Option<Timestamp>,
    NotificationResultClass,
) {
    let class = match result {
        PushSendResult::Accepted => NotificationResultClass::Accepted,
        PushSendResult::Timeout => NotificationResultClass::Timeout,
        PushSendResult::ConnectionFailed => NotificationResultClass::ConnectionFailed,
        PushSendResult::Expired => NotificationResultClass::SubscriptionExpired,
        PushSendResult::PayloadTooLarge => NotificationResultClass::PayloadTooLarge,
        PushSendResult::RateLimited(_) => NotificationResultClass::RateLimited,
        PushSendResult::ClientError => NotificationResultClass::ClientError,
        PushSendResult::ServerError => NotificationResultClass::ServerError,
    };
    if result == PushSendResult::Accepted {
        return (NotificationDeliveryState::Delivered, None, class);
    }
    let retryable = matches!(
        result,
        PushSendResult::Timeout
            | PushSendResult::ConnectionFailed
            | PushSendResult::RateLimited(_)
            | PushSendResult::ServerError
    );
    if retryable && attempts <= RETRY_MS.len() as u32 {
        let provider_delay = match result {
            PushSendResult::RateLimited(Some(seconds)) => Some(
                i64::try_from(seconds.min(300))
                    .unwrap_or(300)
                    .saturating_mul(1_000),
            ),
            _ => None,
        };
        let delay = provider_delay.unwrap_or(RETRY_MS[attempts.saturating_sub(1) as usize]);
        return (
            NotificationDeliveryState::Retrying,
            Some(Timestamp(now.0.saturating_add(delay))),
            class,
        );
    }
    (NotificationDeliveryState::PermanentlyFailed, None, class)
}

fn payload_for(delivery: &PushDelivery, machine: &str) -> Result<SafePushPayload, &'static str> {
    let label = delivery
        .intent
        .session_label
        .as_deref()
        .unwrap_or("Session");
    let (body, priority) = facts_text(&delivery.intent.facts);
    let route = format!(
        "/app/sessions/{}%3A{}",
        encode_component(&delivery.machine_id.0),
        delivery.session_id.0
    );
    SafePushPayload::new(
        &format!("Sentinal · {}", safe_label(machine, "Machine")),
        &format!("{} — {body}", safe_label(label, "Session")),
        delivery.category,
        priority,
        delivery.intent.occurred_at,
        &route,
    )
    .map_err(|_| "safe Push payload could not be built")
}

fn facts_text(facts: &NotificationFacts) -> (String, PushPriority) {
    match facts {
        NotificationFacts::CommandFinished { exit_code: 0 } => {
            ("completed successfully".into(), PushPriority::Default)
        }
        NotificationFacts::CommandFinished { exit_code } => {
            (format!("failed (exit {exit_code})"), PushPriority::High)
        }
        NotificationFacts::Error => ("error detected".into(), PushPriority::High),
        NotificationFacts::Waiting => ("waiting for input".into(), PushPriority::High),
        NotificationFacts::Stalled { silent_for_ms } => (
            format!("possibly stalled ({} min)", silent_for_ms / 60_000),
            PushPriority::Default,
        ),
        NotificationFacts::ResourceHigh { resource, percent } => (
            format!("{} usage high ({percent:.0}%)", resource_name(*resource)),
            PushPriority::High,
        ),
        NotificationFacts::Checkpoint => ("checkpoint created".into(), PushPriority::Low),
        NotificationFacts::Progress {
            current,
            total,
            unit,
        } => (
            format!(
                "{} {}",
                safe_label(unit, "step"),
                total.map_or_else(|| current.to_string(), |total| format!("{current}/{total}"))
            ),
            PushPriority::Low,
        ),
    }
}

fn resource_name(resource: Resource) -> &'static str {
    match resource {
        Resource::Cpu => "CPU",
        Resource::Ram => "RAM",
        Resource::Gpu => "GPU",
        Resource::Vram => "VRAM",
        Resource::Disk => "disk",
    }
}

fn safe_label(value: &str, fallback: &str) -> String {
    let value: String = value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(48)
        .collect();
    if value.is_empty() {
        fallback.into()
    } else {
        value
    }
}

fn encode_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_matrix_is_bounded_and_expiration_is_permanent() {
        let now = Timestamp(1_000);
        assert_eq!(
            classify(PushSendResult::Timeout, 1, now),
            (
                NotificationDeliveryState::Retrying,
                Some(Timestamp(6_000)),
                NotificationResultClass::Timeout
            )
        );
        assert_eq!(
            classify(PushSendResult::RateLimited(Some(999)), 2, now).1,
            Some(Timestamp(301_000))
        );
        assert_eq!(
            classify(PushSendResult::Expired, 1, now),
            (
                NotificationDeliveryState::PermanentlyFailed,
                None,
                NotificationResultClass::SubscriptionExpired
            )
        );
        assert_eq!(
            classify(PushSendResult::ServerError, 5, now).0,
            NotificationDeliveryState::PermanentlyFailed
        );
    }
}
