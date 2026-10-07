//! VAPID-key and untrusted Push-endpoint security tests.

#![allow(clippy::unwrap_used)]

use daemon::push::{
    endpoint::{validate_push_endpoint_shape, validate_resolved_addresses},
    key_store::VapidKeyStore,
};
use protocol::Timestamp;
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    os::unix::fs::{symlink, MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sentinal-push-security-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn vapid_key_is_owner_only_stable_and_redacted() {
    let directory = temp_dir();
    let path = directory.join("push-vapid.json");
    let uid = fs::metadata(&directory).unwrap().uid();
    let first = VapidKeyStore::load_or_create(
        &path,
        "https://control.example.ts.net",
        uid,
        Timestamp(1_000),
    )
    .unwrap();
    let second = VapidKeyStore::load_or_create(
        &path,
        "https://control.example.ts.net",
        uid,
        Timestamp(2_000),
    )
    .unwrap();
    assert_eq!(first.public(), second.public());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let private_key = stored["private_key"].as_str().unwrap();
    let debug = format!("{first:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains(private_key));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn vapid_key_refuses_symlink_corruption_and_open_permissions() {
    let directory = temp_dir();
    let uid = fs::metadata(&directory).unwrap().uid();
    let target = directory.join("target.json");
    fs::write(&target, b"{}").unwrap();
    let link = directory.join("push-vapid.json");
    symlink(&target, &link).unwrap();
    assert!(VapidKeyStore::load_or_create(
        &link,
        "https://control.example.ts.net",
        uid,
        Timestamp(1),
    )
    .is_err());
    fs::remove_file(&link).unwrap();

    fs::write(&link, b"not-json").unwrap();
    fs::set_permissions(&link, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(VapidKeyStore::load_or_create(
        &link,
        "https://control.example.ts.net",
        uid,
        Timestamp(1),
    )
    .is_err());
    fs::set_permissions(&link, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(VapidKeyStore::load_or_create(
        &link,
        "https://control.example.ts.net",
        uid,
        Timestamp(1),
    )
    .is_err());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn endpoint_shape_accepts_only_credential_free_https_dns_port_443() {
    let valid =
        validate_push_endpoint_shape("https://push.example.invalid/wpush/v2/opaque?provider=value")
            .unwrap();
    assert_eq!(valid.host, "push.example.invalid");
    assert_eq!(valid.port, 443);
    for bad in [
        "http://push.example.invalid/a",
        "https://user:pass@push.example.invalid/a",
        "https://127.0.0.1/a",
        "https://[::1]/a",
        "https://push.example.invalid:8443/a",
        "https://push.example.invalid/a#fragment",
        "https://push.example.invalid\\evil",
        "https:///missing-host",
    ] {
        assert!(validate_push_endpoint_shape(bad).is_err(), "{bad}");
    }
}

#[test]
fn resolved_endpoint_addresses_must_all_be_globally_routable() {
    assert!(validate_resolved_addresses(&[
        IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
        IpAddr::V6("2606:4700:4700::1111".parse::<Ipv6Addr>().unwrap()),
    ])
    .is_ok());
    for rejected in [
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.1.1",
        "100.64.0.1",
        "100.127.255.254",
        "192.0.2.1",
        "224.0.0.1",
        "::1",
        "fe80::1",
        "fd7a:115c:a1e0::1",
        "2001:db8::1",
        "ff02::1",
    ] {
        let address: IpAddr = rejected.parse().unwrap();
        assert!(
            validate_resolved_addresses(&[address]).is_err(),
            "{rejected}"
        );
    }
    assert!(validate_resolved_addresses(&[]).is_err());
    assert!(validate_resolved_addresses(&[
        "8.8.8.8".parse().unwrap(),
        "10.0.0.1".parse().unwrap(),
    ])
    .is_err());
}
