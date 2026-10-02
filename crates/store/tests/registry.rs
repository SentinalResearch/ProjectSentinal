//! Control registry, pairing codes, links, and the Machine ID rewrite.

#![allow(clippy::unwrap_used)]

use protocol::{
    link::{LinkEndpoint, MachineCapability, MachineRole},
    Command, CommandId, MachineId, Os, Session, SessionId, Timestamp,
};
use store::{
    registry::{ExchangeRecord, LinkRow, PairingCodeRow, RegistryRow, RegistryState},
    Store,
};

const WORKER: &str = "01J9Z3K8V6Q2T7M4X5C0RNB8DA";
const OTHER: &str = "01J9Z6BBBBBBBBBBBBBBBBBBBB";
const CONTROL: &str = "01J9Y0CTRL0000000000000000";

fn code(code_id: &str, hmac: &str, expires_at: i64) -> PairingCodeRow {
    PairingCodeRow {
        code_id: code_id.into(),
        code_hmac: hmac.into(),
        created_at: Timestamp(0),
        expires_at: Timestamp(expires_at),
        consumed_at: None,
        cancelled_at: None,
        machine_id: None,
    }
}

fn machine(id: &str, name: &str) -> RegistryRow {
    RegistryRow {
        machine_id: MachineId(id.into()),
        display_name: name.into(),
        role: MachineRole::Worker,
        os: Some(Os::Linux),
        daemon_version: Some("0.0.1".into()),
        capabilities: vec![MachineCapability::Sessions],
        endpoint: Some(LinkEndpoint {
            address: "100.74.239.89".into(),
            port: 7420,
            dns_name: None,
        }),
        state: RegistryState::Active,
        paired_at: Timestamp(10),
        last_seen: None,
        last_sequence: 0,
    }
}

fn link(link_id: &str, machine_id: &str, hash: &str) -> LinkRow {
    LinkRow {
        link_id: link_id.into(),
        machine_id: MachineId(machine_id.into()),
        worker_credential_sha256: hash.into(),
        created_at: Timestamp(10),
        expires_at: Timestamp(1_000_000),
        previous_sha256: None,
        previous_valid_until: None,
        revoked_at: None,
        last_used_at: None,
    }
}

fn exchange(hmac: &str, machine_id: &str, name: &str, link_id: &str, hash: &str) -> ExchangeRecord {
    ExchangeRecord {
        code_hmac: hmac.into(),
        now: Timestamp(10),
        machine: machine(machine_id, name),
        link: link(link_id, machine_id, hash),
    }
}

#[test]
fn a_code_is_consumed_exactly_once_with_its_registry_entry() {
    let store = Store::open_in_memory().unwrap();
    store.insert_pairing_code(&code("pc_1", "h1", 100)).unwrap();

    let first = store
        .complete_exchange(&exchange("h1", WORKER, "RTX", "lk_1", "s1"))
        .unwrap();
    let second = store
        .complete_exchange(&exchange("h1", OTHER, "Other", "lk_2", "s2"))
        .unwrap();

    assert_eq!(first.unwrap().code_id, "pc_1");
    assert!(second.is_none(), "a consumed code never works twice");
    assert_eq!(store.registry_machines().unwrap().len(), 1);
    let status = store.pairing_code("pc_1").unwrap().unwrap();
    assert_eq!(status.machine_id, Some(MachineId(WORKER.into())));
}

#[test]
fn expired_cancelled_and_unknown_codes_are_not_consumable() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_pairing_code(&code("pc_exp", "exp", 10))
        .unwrap();
    store
        .insert_pairing_code(&code("pc_can", "can", 100))
        .unwrap();
    assert!(store.cancel_pairing_code("pc_can", Timestamp(5)).unwrap());

    for hmac in ["exp", "can", "missing"] {
        let outcome = store
            .complete_exchange(&exchange(hmac, WORKER, "RTX", "lk", "s"))
            .unwrap();
        assert!(outcome.is_none(), "{hmac}");
    }
    assert!(store.registry_machines().unwrap().is_empty());
}

#[test]
fn outstanding_codes_are_counted_and_cancelled_together() {
    let store = Store::open_in_memory().unwrap();
    store.insert_pairing_code(&code("a", "ha", 100)).unwrap();
    store.insert_pairing_code(&code("b", "hb", 100)).unwrap();
    store.insert_pairing_code(&code("old", "ho", 1)).unwrap();
    assert_eq!(store.count_outstanding_codes(Timestamp(5)).unwrap(), 2);
    assert_eq!(store.count_codes_created_since(Timestamp(0)).unwrap(), 3);

    assert_eq!(store.cancel_outstanding_codes(Timestamp(6)).unwrap(), 2);

    assert_eq!(store.count_outstanding_codes(Timestamp(7)).unwrap(), 0);
    assert!(
        !store.cancel_pairing_code("a", Timestamp(8)).unwrap(),
        "already cancelled"
    );
}

#[test]
fn expired_codes_are_reported_exactly_once() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_pairing_code(&code("pc_old", "h1", 10))
        .unwrap();
    store
        .insert_pairing_code(&code("pc_new", "h2", 100))
        .unwrap();
    assert!(store
        .take_newly_expired_codes(Timestamp(9))
        .unwrap()
        .is_empty());
    assert_eq!(
        store.take_newly_expired_codes(Timestamp(10)).unwrap(),
        vec!["pc_old".to_string()]
    );
    assert!(store
        .take_newly_expired_codes(Timestamp(50))
        .unwrap()
        .is_empty());
}

#[test]
fn pairing_failures_are_counted_in_a_window() {
    let store = Store::open_in_memory().unwrap();
    for at in [1, 2, 50] {
        store.record_pairing_failure(Timestamp(at)).unwrap();
    }
    assert_eq!(store.count_pairing_failures_since(Timestamp(2)).unwrap(), 2);
}

#[test]
fn reconnect_keeps_the_owners_display_name_and_replaces_the_old_link() {
    let store = Store::open_in_memory().unwrap();
    store.insert_pairing_code(&code("pc_1", "h1", 100)).unwrap();
    store.insert_pairing_code(&code("pc_2", "h2", 100)).unwrap();
    store
        .complete_exchange(&exchange("h1", WORKER, "RTX", "lk_1", "s1"))
        .unwrap();
    assert!(store
        .rename_registry_machine(&MachineId(WORKER.into()), "Owner name")
        .unwrap());

    let outcome = store
        .complete_exchange(&exchange("h2", WORKER, "Worker proposal", "lk_2", "s2"))
        .unwrap()
        .unwrap();

    assert_eq!(outcome.replaced_links, vec!["lk_1".to_string()]);
    let entry = store
        .registry_machine(&MachineId(WORKER.into()))
        .unwrap()
        .unwrap();
    assert_eq!(entry.display_name, "Owner name");
    assert!(store
        .active_link_by_hash("s1", Timestamp(20))
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .active_link_by_hash("s2", Timestamp(20))
            .unwrap()
            .unwrap()
            .link_id,
        "lk_2"
    );
}

#[test]
fn revoking_one_machine_leaves_the_other_untouched() {
    let store = Store::open_in_memory().unwrap();
    store.insert_pairing_code(&code("pc_1", "h1", 100)).unwrap();
    store.insert_pairing_code(&code("pc_2", "h2", 100)).unwrap();
    store
        .complete_exchange(&exchange("h1", WORKER, "A", "lk_a", "sa"))
        .unwrap();
    store
        .complete_exchange(&exchange("h2", OTHER, "B", "lk_b", "sb"))
        .unwrap();

    let revoked = store
        .revoke_registry_machine(&MachineId(WORKER.into()), Timestamp(30))
        .unwrap();

    assert_eq!(revoked, Some(vec!["lk_a".to_string()]));
    assert!(store
        .active_link_by_hash("sa", Timestamp(31))
        .unwrap()
        .is_none());
    assert!(store
        .active_link_by_hash("sb", Timestamp(31))
        .unwrap()
        .is_some());
    let states: Vec<_> = store
        .registry_machines()
        .unwrap()
        .into_iter()
        .map(|row| (row.machine_id.0, row.state))
        .collect();
    assert!(states.contains(&(WORKER.into(), RegistryState::Revoked)));
    assert!(states.contains(&(OTHER.into(), RegistryState::Active)));
    assert_eq!(
        store
            .revoke_registry_machine(
                &MachineId("01J9ZZZZZZZZZZZZZZZZZZZZZZ".into()),
                Timestamp(32)
            )
            .unwrap(),
        None
    );
}

#[test]
fn heartbeats_require_a_strictly_increasing_sequence() {
    let store = Store::open_in_memory().unwrap();
    store.insert_pairing_code(&code("pc_1", "h1", 100)).unwrap();
    store
        .complete_exchange(&exchange("h1", WORKER, "A", "lk_a", "sa"))
        .unwrap();
    let id = MachineId(WORKER.into());
    let endpoint = machine(WORKER, "A").endpoint.unwrap();

    assert!(store
        .record_heartbeat(&id, "lk_a", 1, Timestamp(40), &endpoint, "0.0.2", &[])
        .unwrap());
    assert!(!store
        .record_heartbeat(&id, "lk_a", 1, Timestamp(41), &endpoint, "0.0.2", &[])
        .unwrap());
    assert!(!store
        .record_heartbeat(&id, "lk_a", 0, Timestamp(42), &endpoint, "0.0.2", &[])
        .unwrap());
    assert!(store
        .record_heartbeat(&id, "lk_a", 5, Timestamp(43), &endpoint, "0.0.2", &[])
        .unwrap());

    let entry = store.registry_machine(&id).unwrap().unwrap();
    assert_eq!(
        (entry.last_sequence, entry.last_seen),
        (5, Some(Timestamp(43)))
    );
    assert_eq!(entry.daemon_version.as_deref(), Some("0.0.2"));
    let link = store
        .active_link_by_hash("sa", Timestamp(44))
        .unwrap()
        .unwrap();
    assert_eq!(link.last_used_at, Some(Timestamp(43)));
}

#[test]
fn rotated_credentials_honour_a_short_grace_for_the_previous_hash() {
    let store = Store::open_in_memory().unwrap();
    store.insert_pairing_code(&code("pc_1", "h1", 100)).unwrap();
    store
        .complete_exchange(&exchange("h1", WORKER, "A", "lk_a", "old"))
        .unwrap();

    store
        .rotate_link(
            "lk_a",
            "new",
            Timestamp(2_000_000),
            Timestamp(60),
            Timestamp(50),
        )
        .unwrap();

    assert!(store
        .active_link_by_hash("new", Timestamp(55))
        .unwrap()
        .is_some());
    assert!(store
        .active_link_by_hash("old", Timestamp(59))
        .unwrap()
        .is_some());
    assert!(store
        .active_link_by_hash("old", Timestamp(60))
        .unwrap()
        .is_none());
    assert!(
        store
            .active_link_by_hash("new", Timestamp(2_000_000))
            .unwrap()
            .is_none(),
        "expired"
    );
}

#[test]
fn the_control_registers_itself_without_a_link() {
    let store = Store::open_in_memory().unwrap();
    let mut control = machine(CONTROL, "Mac mini");
    control.role = MachineRole::Control;
    control.endpoint = None;
    store.upsert_control_machine(&control).unwrap();
    store
        .rename_registry_machine(&MachineId(CONTROL.into()), "Desk")
        .unwrap();
    store.upsert_control_machine(&control).unwrap();

    let entry = store
        .registry_machine(&MachineId(CONTROL.into()))
        .unwrap()
        .unwrap();
    assert_eq!(
        entry.display_name, "Desk",
        "restart keeps the owner's rename"
    );
    assert_eq!(entry.role, MachineRole::Control);
}

#[test]
fn disconnect_marks_the_machine_and_revokes_its_links() {
    let store = Store::open_in_memory().unwrap();
    store.insert_pairing_code(&code("pc_1", "h1", 100)).unwrap();
    store
        .complete_exchange(&exchange("h1", WORKER, "A", "lk_a", "sa"))
        .unwrap();
    store
        .disconnect_registry_machine(&MachineId(WORKER.into()), Timestamp(70))
        .unwrap();
    let entry = store
        .registry_machine(&MachineId(WORKER.into()))
        .unwrap()
        .unwrap();
    assert_eq!(entry.state, RegistryState::Disconnected);
    assert!(store
        .active_link_by_hash("sa", Timestamp(71))
        .unwrap()
        .is_none());
}

#[test]
fn legacy_machine_id_is_rewritten_in_sessions_and_commands() {
    let store = Store::open_in_memory().unwrap();
    let session = Session::new(
        SessionId(1),
        MachineId("old-host".into()),
        "/bin/zsh",
        "/work",
        Timestamp(1),
    );
    store.insert_session(&session).unwrap();
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("old-host".into()),
        "ls",
        "/work",
        Timestamp(2),
    );
    store.insert_command(&command).unwrap();

    let changed = store
        .rewrite_machine_id("old-host", &MachineId(WORKER.into()))
        .unwrap();

    assert_eq!(changed, 2);
    assert_eq!(
        store.session(SessionId(1)).unwrap().unwrap().machine_id.0,
        WORKER
    );
    assert_eq!(
        store.history(Some(SessionId(1)), 10).unwrap()[0]
            .machine_id
            .0,
        WORKER
    );
    assert_eq!(
        store
            .rewrite_machine_id("old-host", &MachineId(WORKER.into()))
            .unwrap(),
        0
    );
}
