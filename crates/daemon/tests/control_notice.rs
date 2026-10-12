//! Control-originated release notices: sent directly to every active browser
//! subscription (no Session), and only to the one fixed Settings route.

#![allow(clippy::unwrap_used)]

use base64ct::{Base64UrlUnpadded, Encoding as _};
use daemon::push::{
    delivery::{PushSendFuture, PushSendResult, PushTransport},
    gateway::{PushGateway, PushGatewaySettings, ResolveFuture},
    provider::{BuiltPushRequest, PushPriority, SafePushPayload},
};
use protocol::{
    NotificationCategory, PushProviderMode, PushSubscriptionKeys, PushSubscriptionRequest,
    Timestamp,
};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    os::unix::fs::MetadataExt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use store::Store;
use web_push_native::p256::{elliptic_curve::sec1::ToEncodedPoint, SecretKey};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct CountingTransport(Arc<AtomicU64>);
impl PushTransport for CountingTransport {
    fn send(&self, request: BuiltPushRequest, _addresses: Vec<IpAddr>) -> PushSendFuture {
        assert!(!request.body().is_empty());
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { PushSendResult::Accepted })
    }
}

fn gateway(mode: PushProviderMode, devices: usize) -> (PushGateway, Arc<AtomicU64>) {
    let directory = std::env::temp_dir().join(format!(
        "sentinal-control-notice-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    let uid = fs::metadata(&directory).unwrap().uid();
    let store = Arc::new(Store::open_in_memory().unwrap());
    for index in 0..devices {
        let secret = SecretKey::from_slice(&[7_u8 + index as u8; 32]).unwrap();
        let request = PushSubscriptionRequest {
            device_label: format!("Phone {index}"),
            endpoint: format!("https://push.example.invalid/opaque-{index}"),
            expiration_time: None,
            keys: PushSubscriptionKeys {
                p256dh: Base64UrlUnpadded::encode_string(
                    secret.public_key().to_encoded_point(false).as_bytes(),
                ),
                auth: Base64UrlUnpadded::encode_string(&[9_u8; 16]),
            },
        };
        store
            .upsert_push_subscription(
                &format!("ps_{index:016x}"),
                &format!("hash{index}"),
                &request,
                Timestamp(1_000),
                16,
            )
            .unwrap();
    }
    store
        .set_push_provider_mode(mode, Timestamp(1_000))
        .unwrap();
    let attempts = Arc::new(AtomicU64::new(0));
    let resolver = Arc::new(|_: String, _: u16| -> ResolveFuture {
        Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))]) })
    });
    let gateway = PushGateway::with_dependencies(
        store,
        PushGatewaySettings {
            key_file: directory.join("vapid.json"),
            subject_origin: "https://control.example.ts.net".into(),
            owner_uid: uid,
        },
        Arc::new(|| Timestamp(2_000)),
        resolver,
        Arc::new(CountingTransport(attempts.clone())),
    );
    (gateway, attempts)
}

#[tokio::test]
async fn a_release_notice_reaches_every_active_device_only_when_web_push_is_on() {
    let (on, sent) = gateway(PushProviderMode::WebPush, 2);
    let delivered = on
        .send_control_notice(
            "Sentinal 0.1.18 is available",
            "Open Settings → Machines to update this control.",
        )
        .await;
    assert_eq!(delivered, 2);
    assert_eq!(sent.load(Ordering::SeqCst), 2);

    let (off, sent) = gateway(PushProviderMode::Disabled, 1);
    assert_eq!(
        off.send_control_notice("Sentinal 0.1.18 is available", "x")
            .await,
        0
    );
    assert_eq!(sent.load(Ordering::SeqCst), 0);
}

#[test]
fn notice_payloads_open_only_the_machines_settings_page() {
    let build = |route: &str| {
        SafePushPayload::new(
            "Sentinal",
            "Control updated",
            NotificationCategory::Release,
            PushPriority::Default,
            Timestamp(1),
            route,
        )
    };
    assert!(build("/app/settings/machines").is_ok());
    assert!(build("/app/sessions/mbp%3A4").is_ok());
    for bad in [
        "/app/settings/account",
        "/app/settings/machines/x",
        "/app/settings",
        "https://evil.example/app/settings/machines",
    ] {
        assert!(build(bad).is_err(), "{bad}");
    }
}
