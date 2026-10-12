//! Control-originated release notices (4.E §11b): best-effort Web Push to
//! every active browser subscription. They carry no Session, open only the
//! Machines settings page, and are not journaled or retried; the release
//! watch sends each one once.

use super::{
    endpoint::{validate_push_endpoint_shape, validate_resolved_addresses},
    gateway::PushGateway,
    provider::{PushPriority, SafePushPayload, WebPushProvider, RELEASE_NOTICE_ROUTE},
};
use protocol::{NotificationCategory, PushProviderMode};

impl PushGateway {
    /// Send one release notice to every active device; returns how many the
    /// browser Push services accepted. Does nothing unless Web Push is selected.
    pub async fn send_control_notice(&self, title: &str, body: &str) -> usize {
        if self.provider_mode() != PushProviderMode::WebPush {
            return 0;
        }
        let Ok(payload) = SafePushPayload::new(
            title,
            body,
            NotificationCategory::Release,
            PushPriority::Default,
            self.now(),
            RELEASE_NOTICE_ROUTE,
        ) else {
            tracing::warn!("release notice could not be built");
            return 0;
        };
        let (Ok(key), Ok(subscriptions)) =
            (self.signing_key(), self.store().active_push_subscriptions())
        else {
            tracing::warn!("release notice skipped: Push key or subscriptions unavailable");
            return 0;
        };
        let mut accepted = 0;
        for subscription in subscriptions {
            let Ok(shape) = validate_push_endpoint_shape(&subscription.endpoint) else {
                continue;
            };
            let Ok(addresses) = (self.resolver())(shape.host, shape.port).await else {
                continue;
            };
            if validate_resolved_addresses(&addresses).is_err() {
                continue;
            }
            let Ok(request) = WebPushProvider::build_request(
                &subscription.endpoint,
                &subscription.keys.p256dh,
                &subscription.keys.auth,
                &payload,
                &key,
            ) else {
                continue;
            };
            if self.transport().send(request, addresses).await
                == super::delivery::PushSendResult::Accepted
            {
                accepted += 1;
            }
        }
        tracing::info!(accepted, "release notice sent");
        accepted
    }
}
