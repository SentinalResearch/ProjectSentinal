//! Native Push fan-out and injected provider behavior.

#![allow(clippy::unwrap_used)]

use base64ct::{Base64UrlUnpadded, Encoding as _};
use daemon::push::{
    delivery::{PushSendFuture, PushSendResult, PushTransport},
    gateway::{PushGateway, PushGatewaySettings, ResolveFuture},
    provider::BuiltPushRequest,
};
use protocol::{
    EventId, MachineId, NotificationFacts, NotificationIntent, NotificationIntentBatch,
    PushProviderMode, PushSubscriptionKeys, PushSubscriptionRequest, SessionId, Timestamp,
};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use store::Store;
use web_push_native::p256::{elliptic_curve::sec1::ToEncodedPoint, SecretKey};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct AcceptedTransport(Arc<AtomicU64>);
impl PushTransport for AcceptedTransport {
    fn send(&self, request: BuiltPushRequest, addresses: Vec<IpAddr>) -> PushSendFuture {
        assert_eq!(request.host(), "push.example.invalid");
        assert_eq!(addresses, vec![IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))]);
        assert!(request.header("authorization").is_some());
        assert!(!request.body().is_empty());
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { PushSendResult::Accepted })
    }
}

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sentinal-push-delivery-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[tokio::test]
async fn accepted_intent_is_encrypted_and_delivered_once_to_each_device() {
    let directory = temp_dir();
    let uid = fs::metadata(&directory).unwrap().uid();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secret = SecretKey::from_slice(&[7_u8; 32]).unwrap();
    let p256dh =
        Base64UrlUnpadded::encode_string(secret.public_key().to_encoded_point(false).as_bytes());
    let request = PushSubscriptionRequest {
        device_label: "Phone".into(),
        endpoint: "https://push.example.invalid/opaque".into(),
        expiration_time: None,
        keys: PushSubscriptionKeys {
            p256dh,
            auth: Base64UrlUnpadded::encode_string(&[9_u8; 16]),
        },
    };
    store
        .upsert_push_subscription(
            "ps_0123456789abcdef",
            "hash",
            &request,
            Timestamp(1_000),
            16,
        )
        .unwrap();
    store
        .set_push_provider_mode(PushProviderMode::WebPush, Timestamp(1_000))
        .unwrap();
    let machine_id = MachineId("01J9Y0WORKER00000000000000".into());
    let intent = NotificationIntent {
        intent_id: format!("ni_{}", "a".repeat(64)),
        event_id: EventId(7),
        session_id: SessionId(4),
        occurred_at: Timestamp(1_000),
        session_label: Some("Training".into()),
        facts: NotificationFacts::Waiting,
    };
    store
        .accept_notification_intents(
            &NotificationIntentBatch {
                machine_id,
                sent_at: Timestamp(1_000),
                intents: vec![intent],
            },
            Timestamp(1_000),
            10_000,
            10_000,
        )
        .unwrap();
    let resolver = Arc::new(|_: String, _: u16| -> ResolveFuture {
        Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))]) })
    });
    let attempts = Arc::new(AtomicU64::new(0));
    let gateway = PushGateway::with_dependencies(
        store.clone(),
        PushGatewaySettings {
            key_file: directory.join("vapid.json"),
            subject_origin: "https://control.example.ts.net".into(),
            owner_uid: uid,
        },
        Arc::new(|| Timestamp(2_000)),
        resolver,
        Arc::new(AcceptedTransport(attempts.clone())),
    );

    assert_eq!(gateway.deliver_due_once().await.unwrap(), 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(store
        .due_push_deliveries(Timestamp(10_000), 10)
        .unwrap()
        .is_empty());
    assert!(store.push_subscription_infos().unwrap()[0]
        .last_success_at
        .is_some());
    let history = store.push_delivery_history(10).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].state,
        protocol::NotificationDeliveryState::Delivered
    );
    assert_eq!(history[0].attempts, 1);
    assert!(history[0].request_bytes.is_some_and(|bytes| bytes > 0));
    let visible = gateway.delivery_history().unwrap();
    assert_eq!(visible.len(), 1);
    assert!(visible[0].delivered);
    assert!(visible[0].text.contains("waiting for input"));
    let encoded = serde_json::to_string(&visible).unwrap();
    assert!(!encoded.contains("push.example.invalid"));
    assert!(!encoded.contains(&request.keys.auth));
    fs::remove_dir_all(directory).unwrap();
}
