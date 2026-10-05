//! Bounded ntfy HTTP attempts and retry classification.

use super::{
    config::NotifyConfig, NotificationSender, NotificationService, OutboundRequest, SendFuture,
    SendResult, DELIVERY_BATCH, DELIVERY_CONCURRENCY, RETRY_DELAYS_MS,
};
use futures_util::{stream, StreamExt};
use protocol::{NotificationDeliveryState, NotificationResultClass, Timestamp};
use std::{sync::Arc, time::Duration};
use store::NotificationDelivery;

impl NotificationService {
    /// Attempt all currently due deliveries with bounded concurrency.
    pub async fn deliver_due_once(&self) -> Result<usize, String> {
        let _guard = self.delivery_lock.lock().await;
        let due = self
            .store
            .due_notification_deliveries((self.clock)(), DELIVERY_BATCH)
            .map_err(|error| error.to_string())?;
        let results = stream::iter(due.into_iter().map(|record| {
            let service = self.clone();
            async move { service.deliver_one(record).await }
        }))
        .buffer_unordered(DELIVERY_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        let mut delivered = 0;
        for result in results {
            match result {
                Ok(true) => delivered += 1,
                Ok(false) => {}
                Err(error) => tracing::warn!(%error, "notification delivery attempt failed"),
            }
        }
        Ok(delivered)
    }

    async fn deliver_one(&self, record: NotificationDelivery) -> Result<bool, String> {
        let Some(event) = self
            .store
            .notification_event(record.event_id)
            .map_err(|error| error.to_string())?
        else {
            return self.finish_missing(record);
        };
        let session = self
            .store
            .session(record.session_id)
            .map_err(|error| error.to_string())?;
        let title = session.as_ref().and_then(|value| value.title.as_deref());
        let Some(presentation) = super::presentation::render(
            &event.kind,
            &self.machine_name,
            title,
            &self.machine_id,
            record.session_id,
            (self.control_origin)().as_deref(),
        ) else {
            return self.finish_missing(record);
        };
        let config = self
            .config
            .read()
            .ok()
            .and_then(|value| value.config.clone());
        let Some(config) = config else {
            let now = (self.clock)();
            self.store
                .update_notification_delivery(
                    record.id,
                    NotificationDeliveryState::Retrying,
                    record.attempts,
                    Some(Timestamp(now.0.saturating_add(5_000))),
                    None,
                    None,
                    now,
                )
                .map_err(|error| error.to_string())?;
            return Ok(false);
        };
        let request = outbound(config, presentation);
        let request_bytes = request_size(&request);
        let result = (self.sender)(request).await;
        let attempts = record.attempts.saturating_add(1);
        let now = (self.clock)();
        let (state, next, class) = classify(result, attempts, now);
        self.store
            .update_notification_delivery(
                record.id,
                state,
                attempts,
                next,
                Some(class),
                Some(request_bytes),
                now,
            )
            .map_err(|error| error.to_string())?;
        Ok(state == NotificationDeliveryState::Delivered)
    }

    fn finish_missing(&self, record: NotificationDelivery) -> Result<bool, String> {
        self.store
            .update_notification_delivery(
                record.id,
                NotificationDeliveryState::PermanentlyFailed,
                record.attempts,
                None,
                Some(NotificationResultClass::ClientError),
                None,
                (self.clock)(),
            )
            .map_err(|error| error.to_string())?;
        Ok(false)
    }
}

fn outbound(config: NotifyConfig, presentation: super::Presentation) -> OutboundRequest {
    OutboundRequest {
        url: format!("{}/{}", config.server.trim_end_matches('/'), config.topic),
        token: config.token,
        presentation,
    }
}

fn request_size(request: &OutboundRequest) -> u32 {
    let size = request.url.len()
        + request.token.as_ref().map_or(0, String::len)
        + request.presentation.title.len()
        + request.presentation.body.len()
        + request.presentation.tag.len()
        + request.presentation.priority.len()
        + request.presentation.click.as_ref().map_or(0, String::len);
    u32::try_from(size).unwrap_or(u32::MAX)
}

fn classify(
    result: SendResult,
    attempts: u32,
    now: Timestamp,
) -> (
    NotificationDeliveryState,
    Option<Timestamp>,
    NotificationResultClass,
) {
    if result == SendResult::Accepted {
        return (
            NotificationDeliveryState::Delivered,
            None,
            NotificationResultClass::Accepted,
        );
    }
    let (class, retry_after) = match result {
        SendResult::Timeout => (NotificationResultClass::Timeout, None),
        SendResult::ConnectionFailed => (NotificationResultClass::ConnectionFailed, None),
        SendResult::RateLimited(seconds) => (
            NotificationResultClass::RateLimited,
            seconds.map(|value| value.min(300).saturating_mul(1_000) as i64),
        ),
        SendResult::ServerError => (NotificationResultClass::ServerError, None),
        SendResult::ClientError => (NotificationResultClass::ClientError, None),
        SendResult::Accepted => (NotificationResultClass::Accepted, None),
    };
    let retryable = matches!(
        result,
        SendResult::Timeout
            | SendResult::ConnectionFailed
            | SendResult::RateLimited(_)
            | SendResult::ServerError
    );
    if retryable && attempts <= RETRY_DELAYS_MS.len() as u32 {
        let index = attempts.saturating_sub(1) as usize;
        let delay = retry_after.unwrap_or(RETRY_DELAYS_MS[index]);
        return (
            NotificationDeliveryState::Retrying,
            Some(Timestamp(now.0.saturating_add(delay))),
            class,
        );
    }
    (NotificationDeliveryState::PermanentlyFailed, None, class)
}

pub(super) fn reqwest_sender() -> (NotificationSender, Option<String>) {
    match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => {
            let sender = Arc::new(move |request: OutboundRequest| {
                let client = client.clone();
                Box::pin(async move {
                    let mut builder = client
                        .post(request.url)
                        .header("content-type", "text/plain; charset=utf-8")
                        .header("title", request.presentation.title)
                        .header("tags", request.presentation.tag)
                        .header("priority", request.presentation.priority)
                        .body(request.presentation.body);
                    if let Some(click) = request.presentation.click {
                        builder = builder.header("click", click);
                    }
                    if let Some(token) = request.token {
                        builder = builder.bearer_auth(token);
                    }
                    match builder.send().await {
                        Ok(response) if response.status().is_success() => SendResult::Accepted,
                        Ok(response) if response.status().as_u16() == 429 => {
                            let retry = response
                                .headers()
                                .get("retry-after")
                                .and_then(|value| value.to_str().ok())
                                .and_then(|value| value.parse::<u64>().ok());
                            SendResult::RateLimited(retry)
                        }
                        Ok(response)
                            if matches!(response.status().as_u16(), 408 | 425)
                                || response.status().is_server_error() =>
                        {
                            SendResult::ServerError
                        }
                        Ok(_) => SendResult::ClientError,
                        Err(error) if error.is_timeout() => SendResult::Timeout,
                        Err(_) => SendResult::ConnectionFailed,
                    }
                }) as SendFuture
            });
            (sender, None)
        }
        Err(error) => {
            let sender = Arc::new(|_: OutboundRequest| {
                Box::pin(async { SendResult::ConnectionFailed }) as SendFuture
            });
            (
                sender,
                Some(format!(
                    "notification HTTP client could not be built: {error}"
                )),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_matrix_is_bounded() {
        let now = Timestamp(1_000);
        assert_eq!(
            classify(SendResult::Timeout, 1, now),
            (
                NotificationDeliveryState::Retrying,
                Some(Timestamp(6_000)),
                NotificationResultClass::Timeout
            )
        );
        assert_eq!(
            classify(SendResult::RateLimited(Some(999)), 2, now).1,
            Some(Timestamp(301_000))
        );
        assert_eq!(
            classify(SendResult::ServerError, 4, now).0,
            NotificationDeliveryState::PermanentlyFailed
        );
        assert_eq!(
            classify(SendResult::ClientError, 1, now).0,
            NotificationDeliveryState::PermanentlyFailed
        );
    }
}
