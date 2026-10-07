//! Encrypted Web Push request construction and privacy tests.

#![allow(clippy::unwrap_used)]

use base64ct::{Base64UrlUnpadded, Encoding as _};
use daemon::push::{
    key_store::VapidKeyStore,
    provider::{PushPriority, SafePushPayload, WebPushProvider},
};
use protocol::{NotificationCategory, Timestamp};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use web_push_native::{
    decrypt,
    p256::{elliptic_curve::sec1::ToEncodedPoint, SecretKey},
    Auth,
};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sentinal-web-push-provider-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn request_encrypts_only_the_closed_safe_payload() {
    let directory = temp_dir();
    let uid = fs::metadata(&directory).unwrap().uid();
    let vapid = VapidKeyStore::load_or_create(
        &directory.join("push-vapid.json"),
        "https://control.example.ts.net",
        uid,
        Timestamp(1_000),
    )
    .unwrap();
    let ua_secret = SecretKey::from_slice(&[7_u8; 32]).unwrap();
    let p256dh =
        Base64UrlUnpadded::encode_string(ua_secret.public_key().to_encoded_point(false).as_bytes());
    let auth_bytes = [9_u8; 16];
    let auth = Base64UrlUnpadded::encode_string(&auth_bytes);
    let payload = SafePushPayload::new(
        "Sentinal · RTX PC",
        "Training — waiting for input",
        NotificationCategory::Waiting,
        PushPriority::High,
        Timestamp(2_000),
        "/app/sessions/01JWORKER%3A7",
    )
    .unwrap();
    let request = WebPushProvider::build_request(
        "https://push.example.invalid/wpush/opaque",
        &p256dh,
        &auth,
        &payload,
        &vapid,
    )
    .unwrap();

    assert_eq!(request.host(), "push.example.invalid");
    assert_eq!(request.header("content-encoding"), Some("aes128gcm"));
    assert_eq!(
        request.header("content-type"),
        Some("application/octet-stream")
    );
    assert!(request.header("authorization").is_some());
    assert!(!request
        .body()
        .windows("waiting".len())
        .any(|part| part == b"waiting"));

    let decrypted = decrypt(
        request.body().to_vec(),
        &ua_secret,
        &Auth::clone_from_slice(&auth_bytes),
    )
    .unwrap();
    let decoded: SafePushPayload = serde_json::from_slice(&decrypted).unwrap();
    assert_eq!(decoded, payload);
    let json = String::from_utf8(decrypted).unwrap();
    for forbidden in ["python train.py", "/Users/", "credential", "endpoint"] {
        assert!(!json.contains(forbidden));
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn payload_rejects_external_routes_controls_and_oversize() {
    let build = |title: &str, body: &str, route: &str| {
        SafePushPayload::new(
            title,
            body,
            NotificationCategory::Waiting,
            PushPriority::High,
            Timestamp(1),
            route,
        )
    };
    assert!(build("Sentinal", "waiting", "/app/sessions/machine%3A1").is_ok());
    assert!(build("Sentinal", "waiting", "https://evil.example/x").is_err());
    assert!(build("bad\ntitle", "waiting", "/app/sessions/m%3A1").is_err());
    assert!(build("Sentinal", &"x".repeat(2_100), "/app/sessions/m%3A1").is_err());
}
