//! Approval pairing requests (migration 010): verification gates approval,
//! the secret gates claim and cancel, and a claim succeeds exactly once in
//! the same transaction as the registry and link writes.

#![allow(clippy::unwrap_used)]

use protocol::{
    link::{LinkEndpoint, LinkMachineInfo, MachineCapability, MachineRole},
    pairing::{PairingRequestState, TailscaleIdentity},
    MachineId, Os, Timestamp,
};
use store::{
    pairing_requests::PairingRequestRow,
    registry::{LinkRow, RegistryRow, RegistryState},
    Store,
};

const WORKER: &str = "01J9Z3K8V6Q2T7M4X5C0RNB8DA";
const SECRET_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn request(id: &str, created_at: i64) -> PairingRequestRow {
    PairingRequestRow {
        request_id: id.into(),
        created_at: Timestamp(created_at),
        expires_at: Timestamp(created_at + 600_000),
        state: PairingRequestState::Pending,
        secret_sha256: SECRET_HASH.into(),
        worker_nonce: "11".repeat(16),
        control_nonce: "22".repeat(16),
        claimed: LinkMachineInfo {
            machine_id: MachineId(WORKER.into()),
            display_name: "Claimed name".into(),
            os: Os::Linux,
            daemon_version: "0.1.18".into(),
            capabilities: vec![MachineCapability::Sessions],
        },
        endpoint: LinkEndpoint {
            address: "100.101.0.7".into(),
            port: 7420,
            dns_name: None,
        },
        tailscale: None,
        proof_ok: false,
        approved_name: None,
        decided_at: None,
        claimed_at: None,
    }
}

fn identity() -> TailscaleIdentity {
    TailscaleIdentity {
        node: "worker".into(),
        login: "owner@example.com".into(),
        os: "linux".into(),
        address: "100.101.0.7".into(),
    }
}

fn machine(name: &str) -> RegistryRow {
    RegistryRow {
        machine_id: MachineId(WORKER.into()),
        display_name: name.into(),
        role: MachineRole::Worker,
        os: Some(Os::Linux),
        daemon_version: Some("0.1.18".into()),
        capabilities: vec![MachineCapability::Sessions],
        endpoint: None,
        state: RegistryState::Active,
        paired_at: Timestamp(10),
        last_seen: None,
        last_sequence: 0,
    }
}

fn link(id: &str, hash: &str) -> LinkRow {
    LinkRow {
        link_id: id.into(),
        machine_id: MachineId(WORKER.into()),
        worker_credential_sha256: hash.into(),
        created_at: Timestamp(10),
        expires_at: Timestamp(10_000_000),
        previous_sha256: None,
        previous_valid_until: None,
        revoked_at: None,
        last_used_at: None,
    }
}

#[test]
fn only_a_verified_request_can_be_approved_and_it_is_claimed_once() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_pairing_request(&request("pr_0000000000000001", 1_000))
        .unwrap();
    assert!(
        !store
            .decide_pairing_request("pr_0000000000000001", Some("Lab"), Timestamp(2_000))
            .unwrap(),
        "unverified"
    );
    assert!(store
        .verify_pairing_request("pr_0000000000000001", &identity(), Timestamp(2_000))
        .unwrap());
    assert!(store
        .decide_pairing_request("pr_0000000000000001", Some("Lab"), Timestamp(3_000))
        .unwrap());
    let row = store
        .pairing_request("pr_0000000000000001")
        .unwrap()
        .unwrap();
    assert_eq!(row.state, PairingRequestState::Approved);
    assert_eq!(row.tailscale, Some(identity()));

    let wrong = store
        .claim_pairing_request(
            "pr_0000000000000001",
            &"b".repeat(64),
            &machine("Lab"),
            &link("lk_1", "h1"),
            Timestamp(4_000),
        )
        .unwrap();
    assert!(wrong.is_none(), "wrong secret");
    let claimed = store
        .claim_pairing_request(
            "pr_0000000000000001",
            SECRET_HASH,
            &machine("Lab"),
            &link("lk_1", "h1"),
            Timestamp(4_000),
        )
        .unwrap()
        .unwrap();
    assert_eq!(claimed.display_name, "Lab");
    assert!(store
        .active_link_for(&MachineId(WORKER.into()))
        .unwrap()
        .is_some());
    let again = store
        .claim_pairing_request(
            "pr_0000000000000001",
            SECRET_HASH,
            &machine("Lab"),
            &link("lk_2", "h2"),
            Timestamp(4_001),
        )
        .unwrap();
    assert!(again.is_none(), "one success only");
    assert_eq!(
        store
            .pairing_request("pr_0000000000000001")
            .unwrap()
            .unwrap()
            .state,
        PairingRequestState::Claimed
    );
}

#[test]
fn approval_renames_a_known_machine_and_replaces_its_old_link() {
    let store = Store::open_in_memory().unwrap();
    for (id, name, hash) in [
        ("pr_0000000000000001", "Old", "h1"),
        ("pr_0000000000000002", "New", "h2"),
    ] {
        store.insert_pairing_request(&request(id, 1_000)).unwrap();
        store
            .verify_pairing_request(id, &identity(), Timestamp(2_000))
            .unwrap();
        store
            .decide_pairing_request(id, Some(name), Timestamp(3_000))
            .unwrap();
        store
            .claim_pairing_request(
                id,
                SECRET_HASH,
                &machine(name),
                &link(&format!("lk_{hash}"), hash),
                Timestamp(4_000),
            )
            .unwrap()
            .unwrap();
    }
    let active = store
        .active_link_for(&MachineId(WORKER.into()))
        .unwrap()
        .unwrap();
    assert_eq!(active.link_id, "lk_h2");
    let registered = store
        .registry_machine(&MachineId(WORKER.into()))
        .unwrap()
        .unwrap();
    assert_eq!(registered.display_name, "New");
}

#[test]
fn expired_rejected_and_cancelled_requests_cannot_be_approved_or_claimed() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_pairing_request(&request("pr_0000000000000001", 1_000))
        .unwrap();
    store
        .verify_pairing_request("pr_0000000000000001", &identity(), Timestamp(2_000))
        .unwrap();
    let late = 1_000 + 600_000;
    assert!(!store
        .decide_pairing_request("pr_0000000000000001", Some("Lab"), Timestamp(late))
        .unwrap());
    let row = store
        .pairing_request("pr_0000000000000001")
        .unwrap()
        .unwrap();
    assert_eq!(row.state_at(Timestamp(late)), PairingRequestState::Expired);
    assert!(!store
        .verify_pairing_request("pr_0000000000000001", &identity(), Timestamp(late))
        .unwrap());

    store
        .insert_pairing_request(&request("pr_0000000000000002", 1_000))
        .unwrap();
    store
        .verify_pairing_request("pr_0000000000000002", &identity(), Timestamp(2_000))
        .unwrap();
    assert!(store
        .decide_pairing_request("pr_0000000000000002", None, Timestamp(3_000))
        .unwrap());
    assert!(!store
        .decide_pairing_request("pr_0000000000000002", Some("Lab"), Timestamp(3_001))
        .unwrap());

    store
        .insert_pairing_request(&request("pr_0000000000000003", 1_000))
        .unwrap();
    assert!(!store
        .cancel_pairing_request("pr_0000000000000003", &"b".repeat(64), Timestamp(2_000))
        .unwrap());
    assert!(store
        .cancel_pairing_request("pr_0000000000000003", SECRET_HASH, Timestamp(2_000))
        .unwrap());
    store
        .verify_pairing_request("pr_0000000000000003", &identity(), Timestamp(2_000))
        .unwrap();
    assert!(!store
        .decide_pairing_request("pr_0000000000000003", Some("Lab"), Timestamp(3_000))
        .unwrap());
    for id in ["pr_0000000000000002", "pr_0000000000000003"] {
        assert!(store
            .claim_pairing_request(
                id,
                SECRET_HASH,
                &machine("Lab"),
                &link(id, id),
                Timestamp(4_000)
            )
            .unwrap()
            .is_none());
    }
    assert!(store
        .active_link_for(&MachineId(WORKER.into()))
        .unwrap()
        .is_none());
}

#[test]
fn counts_and_lockout_cancel_support_the_rate_limits() {
    let store = Store::open_in_memory().unwrap();
    for index in 0..3 {
        store
            .insert_pairing_request(&request(
                &format!("pr_000000000000000{index}"),
                1_000 + index,
            ))
            .unwrap();
    }
    assert_eq!(
        store
            .count_pending_pairing_requests(Timestamp(2_000))
            .unwrap(),
        3
    );
    assert_eq!(
        store
            .count_pairing_requests_since(Timestamp(1_001))
            .unwrap(),
        2
    );
    let recent = store.recent_pairing_requests(Timestamp(0), 2).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].request_id, "pr_0000000000000002", "newest first");
    store.record_claim_failure(Timestamp(5)).unwrap();
    store.record_claim_failure(Timestamp(9)).unwrap();
    assert_eq!(store.count_claim_failures_since(Timestamp(6)).unwrap(), 1);
    assert_eq!(
        store
            .cancel_open_pairing_requests(Timestamp(3_000))
            .unwrap(),
        3
    );
    assert_eq!(
        store
            .count_pending_pairing_requests(Timestamp(3_000))
            .unwrap(),
        0
    );
}
