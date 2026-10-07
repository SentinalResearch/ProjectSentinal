//! Compile-time-only Web Push transport for offline browser acceptance.

use super::{
    delivery::{PushSendFuture, PushSendResult, PushTransport},
    gateway::{PushGateway, PushGatewaySettings, ResolveFuture},
    provider::BuiltPushRequest,
};
use protocol::Timestamp;
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};
use store::Store;

struct AcceptedEncryptedTransport;

impl PushTransport for AcceptedEncryptedTransport {
    fn send(&self, request: BuiltPushRequest, addresses: Vec<IpAddr>) -> PushSendFuture {
        let is_encrypted = request.header("authorization").is_some()
            && request.header("content-encoding") == Some("aes128gcm")
            && !request.body().is_empty();
        let is_pinned = addresses == [IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))];
        Box::pin(async move {
            if is_encrypted && is_pinned {
                PushSendResult::Accepted
            } else {
                PushSendResult::ClientError
            }
        })
    }
}

/// Build an offline gateway that still validates and encrypts every request.
pub(crate) fn accepted_gateway(
    store: Arc<Store>,
    settings: PushGatewaySettings,
    clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
) -> PushGateway {
    let resolver = Arc::new(|_: String, _: u16| -> ResolveFuture {
        Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))]) })
    });
    PushGateway::with_dependencies(
        store,
        settings,
        clock,
        resolver,
        Arc::new(AcceptedEncryptedTransport),
    )
}
