//! Control-Machine pairing codes, exchange, heartbeat, registry, and revocation.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::control::{ControlService, ControlSettings, RefreshFault};
use protocol::{
    link::{
        LinkEndpoint, LinkExchangeRequest, LinkFailureCode, LinkHeartbeatRequest, LinkMachineInfo,
        LinkRefreshRequest, MachineCapability, PairingCodeState, RegistryStatus,
    },
    CaptureLevel, MachineId, Os, ProjectedSession, ProjectionBatch, ProjectionChange, Secret,
    Session, SessionId, Timestamp, PROJECTION_SCHEMA_VERSION,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc,
    },
};
use store::Store;

const CONTROL: &str = "01J9Y0CTRL0000000000000000";
const WORKER_A: &str = "01J9Z3K8V6Q2T7M4X5C0RNB8DA";
const WORKER_B: &str = "01J9Z6BBBBBBBBBBBBBBBBBBBB";
const START: i64 = 1_790_900_000_000;
const MINUTE: i64 = 60_000;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Harness {
    control: ControlService,
    store: Arc<Store>,
    clock: Arc<AtomicI64>,
    dir: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn harness() -> Harness {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-control-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let clock = Arc::new(AtomicI64::new(START));
    let control = open(&store, &clock, &dir);
    Harness {
        control,
        store,
        clock,
        dir,
    }
}

fn open(store: &Arc<Store>, clock: &Arc<AtomicI64>, dir: &Path) -> ControlService {
    open_with_read_model(store, clock, dir, true)
}

fn open_with_read_model(
    store: &Arc<Store>,
    clock: &Arc<AtomicI64>,
    dir: &Path,
    read_model_enabled: bool,
) -> ControlService {
    let clock = clock.clone();
    ControlService::open(
        store.clone(),
        ControlSettings {
            public_url: "https://sentinal-control.example.ts.net".into(),
            heartbeat_interval_ms: 30_000,
            development: false,
            machine_id: MachineId(CONTROL.into()),
            display_name: "Mac mini".into(),
            discovery_name: None,
            package_dir: None,
            os: Os::Macos,
            daemon_version: "0.0.1".into(),
            secrets_file: dir.join("machine-credentials.json"),
            audit_dir: dir.to_path_buf(),
            read_model_enabled,
        },
        Arc::new(move || Timestamp(clock.load(Ordering::SeqCst))),
    )
    .unwrap()
}

impl Harness {
    fn advance(&self, ms: i64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
    }

    fn now(&self) -> Timestamp {
        Timestamp(self.clock.load(Ordering::SeqCst))
    }

    fn audit(&self) -> String {
        fs::read_to_string(self.dir.join("pairing-audit.jsonl")).unwrap_or_default()
    }

    fn secrets(&self) -> String {
        fs::read_to_string(self.dir.join("machine-credentials.json")).unwrap()
    }

    fn exchange_request(&self, code: &str, machine_id: &str, name: &str) -> LinkExchangeRequest {
        let credential_digit = if machine_id == WORKER_B { "b" } else { "a" };
        LinkExchangeRequest {
            pairing_code: Secret::new(code),
            machine: LinkMachineInfo {
                machine_id: MachineId(machine_id.into()),
                display_name: name.into(),
                os: Os::Linux,
                daemon_version: "0.0.1".into(),
                capabilities: vec![MachineCapability::Sessions],
            },
            endpoint: endpoint("100.74.239.89"),
            control_credential: Secret::new(credential_digit.repeat(64)),
        }
    }

    /// Create a code and exchange it, returning the worker credential.
    fn pair(&self, machine_id: &str, name: &str) -> Secret {
        let code = self.control.create_code().unwrap();
        self.control
            .exchange(
                self.exchange_request(code.code.expose(), machine_id, name),
                "tester",
            )
            .unwrap()
            .worker_credential
    }

    fn heartbeat(
        &self,
        credential: &Secret,
        machine_id: &str,
        sequence: u64,
    ) -> Result<(), LinkFailureCode> {
        self.control
            .heartbeat(
                credential.expose(),
                LinkHeartbeatRequest {
                    machine_id: MachineId(machine_id.into()),
                    sequence,
                    sent_at: self.now(),
                    endpoint: endpoint("100.74.239.89"),
                    daemon_version: "0.0.1".into(),
                    capabilities: vec![MachineCapability::Sessions],
                    projection_sync: None,
                    update_status: None,
                    health: None,
                },
            )
            .map(|_| ())
            .map_err(|failure| failure.failure.code)
    }
}

fn endpoint(address: &str) -> LinkEndpoint {
    LinkEndpoint {
        address: address.into(),
        port: 7420,
        dns_name: None,
    }
}

fn projection_batch(machine_id: &str) -> ProjectionBatch {
    let mut session = Session::new(
        SessionId(7),
        MachineId(machine_id.into()),
        "/bin/zsh",
        "/private/cwd",
        Timestamp(START),
    );
    session.title = Some("Training".into());
    session.capture = CaptureLevel::Full;
    ProjectionBatch::Delta {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: MachineId(machine_id.into()),
        source_epoch: "pe_test".into(),
        base_cursor: 0,
        through_cursor: 1,
        sent_at: Timestamp(START),
        changes: vec![ProjectionChange::SessionUpsert {
            cursor: 1,
            session: ProjectedSession::from_session(&session),
        }],
    }
}

fn projection_session_batch(machine_id: &str, base_cursor: u64, count: u64) -> ProjectionBatch {
    let changes = (1..=count)
        .map(|offset| {
            let cursor = base_cursor + offset;
            let mut session = Session::new(
                SessionId(cursor),
                MachineId(machine_id.into()),
                "/bin/zsh",
                "/private/cwd",
                Timestamp(START),
            );
            session.title = Some("bounded dashboard session".into());
            ProjectionChange::SessionUpsert {
                cursor,
                session: ProjectedSession::from_session(&session),
            }
        })
        .collect();
    ProjectionBatch::Delta {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: MachineId(machine_id.into()),
        source_epoch: "pe_scale".into(),
        base_cursor,
        through_cursor: base_cursor + count,
        sent_at: Timestamp(START),
        changes,
    }
}

// ---- routing ---------------------------------------------------------------

#[test]
fn heartbeat_negotiates_projection_upload_without_breaking_legacy_workers() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    let mut request = LinkHeartbeatRequest {
        machine_id: MachineId(WORKER_A.into()),
        sequence: 1,
        sent_at: h.now(),
        endpoint: endpoint("100.74.239.89"),
        daemon_version: "0.0.1".into(),
        capabilities: vec![MachineCapability::Sessions],
        projection_sync: Some(protocol::ProjectionSyncReport {
            schema_version: PROJECTION_SCHEMA_VERSION,
            source_epoch: "pe_test".into(),
            current_cursor: 1,
            last_acked_cursor: 0,
            snapshot_required: false,
        }),
        update_status: None,
        health: None,
    };
    let response = h
        .control
        .heartbeat(credential.expose(), request.clone())
        .unwrap();
    let directive = response.projection_sync.unwrap();
    assert_eq!(directive.action, protocol::ProjectionSyncAction::Upload);
    assert_eq!(directive.committed_cursor, 0);

    request.sequence = 2;
    request.projection_sync = None;
    assert_eq!(
        h.control
            .heartbeat(credential.expose(), request.clone())
            .unwrap()
            .projection_sync,
        None
    );

    request.sequence = 3;
    request.projection_sync = Some(protocol::ProjectionSyncReport {
        schema_version: PROJECTION_SCHEMA_VERSION,
        source_epoch: "pe_test".into(),
        current_cursor: 2,
        last_acked_cursor: 0,
        snapshot_required: true,
    });
    assert_eq!(
        h.control
            .heartbeat(credential.expose(), request)
            .unwrap()
            .projection_sync
            .unwrap()
            .action,
        protocol::ProjectionSyncAction::Snapshot
    );
}

#[test]
fn disabled_read_model_refuses_projection_without_deleting_existing_data() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    h.control
        .accept_projection(credential.expose(), projection_batch(WORKER_A))
        .unwrap();

    let disabled = open_with_read_model(&h.store, &h.clock, &h.dir, false);
    let response = disabled
        .heartbeat(
            credential.expose(),
            LinkHeartbeatRequest {
                machine_id: MachineId(WORKER_A.into()),
                sequence: 1,
                sent_at: h.now(),
                endpoint: endpoint("100.74.239.89"),
                daemon_version: "0.0.1".into(),
                capabilities: vec![MachineCapability::Sessions],
                projection_sync: Some(protocol::ProjectionSyncReport {
                    schema_version: PROJECTION_SCHEMA_VERSION,
                    source_epoch: "pe_test".into(),
                    current_cursor: 1,
                    last_acked_cursor: 0,
                    snapshot_required: false,
                }),
                update_status: None,
                health: None,
            },
        )
        .unwrap();
    let directive = response.projection_sync.unwrap();
    assert!(!directive.enabled);
    assert_eq!(directive.action, protocol::ProjectionSyncAction::Disabled);

    for failure in [
        disabled.projection_snapshot().unwrap_err(),
        disabled
            .accept_projection(credential.expose(), projection_batch(WORKER_A))
            .unwrap_err(),
    ] {
        assert_eq!(failure.status, 404);
        assert_eq!(failure.failure.code, LinkFailureCode::NotFound);
    }
    disabled
        .accept_local_projection(projection_batch(CONTROL))
        .unwrap();
    assert_eq!(
        h.store
            .projected_sessions(&MachineId(WORKER_A.into()), 10)
            .unwrap()
            .len(),
        1,
        "disabling the read model must preserve disposable projection rows"
    );
    assert_eq!(
        h.store
            .projected_sessions(&MachineId(CONTROL.into()), 10)
            .unwrap()
            .len(),
        1,
        "the hidden local projection must stay current for a safe re-enable"
    );
}

#[test]
fn projection_ingress_is_authenticated_machine_bound_and_idempotent() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    let batch = projection_batch(WORKER_A);
    let first = h
        .control
        .accept_projection(credential.expose(), batch.clone())
        .unwrap();
    assert_eq!(
        first.status,
        protocol::ProjectionAcknowledgementStatus::Accepted
    );
    let duplicate = h
        .control
        .accept_projection(credential.expose(), batch)
        .unwrap();
    assert_eq!(
        duplicate.status,
        protocol::ProjectionAcknowledgementStatus::Duplicate
    );
    let snapshot = h.control.projection_snapshot().unwrap();
    let worker = snapshot
        .machines
        .iter()
        .find(|machine| machine.machine_id == MachineId(WORKER_A.into()))
        .unwrap();
    assert_eq!(worker.sessions[0].title.as_deref(), Some("Training"));

    assert_eq!(
        h.control
            .accept_projection(credential.expose(), projection_batch(WORKER_B))
            .unwrap_err()
            .failure
            .code,
        LinkFailureCode::Unauthorized
    );
}

#[test]
fn owner_snapshot_has_one_global_session_budget_across_machines() {
    let h = harness();
    let first = h.pair(WORKER_A, "A");
    let second = h.pair(WORKER_B, "B");
    for (machine, credential, total) in [(WORKER_A, &first, 600_u64), (WORKER_B, &second, 500_u64)]
    {
        let mut cursor = 0;
        while cursor < total {
            let count = 128_u64.min(total - cursor);
            h.control
                .accept_projection(
                    credential.expose(),
                    projection_session_batch(machine, cursor, count),
                )
                .unwrap();
            cursor += count;
        }
    }

    let snapshot = h.control.projection_snapshot().unwrap();
    assert_eq!(
        snapshot
            .machines
            .iter()
            .map(|machine| machine.sessions.len())
            .sum::<usize>(),
        1_000
    );
    assert_eq!(snapshot.omitted_sessions, 100);
    assert!(snapshot.truncated);
    assert!(serde_json::to_vec(&snapshot).unwrap().len() <= 2 * 1024 * 1024);
}

#[test]
fn projection_reconnect_replays_machine_scoped_revisions() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    h.control
        .accept_projection(credential.expose(), projection_batch(WORKER_A))
        .unwrap();

    let updates = h.control.projection_updates_after(0).unwrap();
    assert_eq!(updates.len(), 1);
    assert!(matches!(
        &updates[0],
        protocol::ControlProjectionMessage::ProjectionDelta {
            machine_id: Some(machine_id),
            from_revision: 0,
            to_revision: 1,
            changes,
        } if *machine_id == MachineId(WORKER_A.into()) && changes.len() == 1
    ));
}

#[test]
fn revoking_a_machine_purges_its_disposable_projection() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    h.control
        .accept_projection(credential.expose(), projection_batch(WORKER_A))
        .unwrap();
    assert_eq!(
        h.store
            .projected_sessions(&MachineId(WORKER_A.into()), 10)
            .unwrap()
            .len(),
        1
    );

    h.control.revoke(&MachineId(WORKER_A.into())).unwrap();

    assert!(h
        .store
        .projected_sessions(&MachineId(WORKER_A.into()), 10)
        .unwrap()
        .is_empty());
    assert!(h
        .store
        .control_projection_state(&MachineId(WORKER_A.into()))
        .unwrap()
        .is_none());
}

#[test]
fn routing_targets_are_machine_scoped_and_revocation_removes_only_one() {
    let h = harness();
    let credential_a = h.pair(WORKER_A, "A");
    let credential_b = h.pair(WORKER_B, "B");
    h.heartbeat(&credential_a, WORKER_A, 1).unwrap();
    h.heartbeat(&credential_b, WORKER_B, 1).unwrap();

    let target_a = h
        .control
        .routing_target(&MachineId(WORKER_A.into()))
        .unwrap();
    let target_b = h
        .control
        .routing_target(&MachineId(WORKER_B.into()))
        .unwrap();
    assert_eq!(target_a.machine_id, MachineId(WORKER_A.into()));
    assert_eq!(target_b.machine_id, MachineId(WORKER_B.into()));
    assert_ne!(
        target_a.control_credential.expose(),
        target_b.control_credential.expose(),
        "each Machine must route with only its own credential"
    );

    h.control.revoke(&MachineId(WORKER_A.into())).unwrap();
    assert_eq!(
        h.control
            .routing_target(&MachineId(WORKER_A.into()))
            .unwrap_err()
            .failure
            .code,
        LinkFailureCode::LinkRevoked
    );
    assert_eq!(
        h.control
            .routing_target(&MachineId(WORKER_B.into()))
            .unwrap()
            .machine_id,
        MachineId(WORKER_B.into())
    );
}

#[test]
fn routing_refuses_an_expired_machine_credential() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    h.heartbeat(&credential, WORKER_A, 1).unwrap();
    h.advance(90 * 24 * 60 * 60 * 1_000);

    assert_eq!(
        h.control
            .routing_target(&MachineId(WORKER_A.into()))
            .unwrap_err()
            .failure
            .code,
        LinkFailureCode::Unauthorized
    );
    let projected = h
        .control
        .projection_snapshot()
        .unwrap()
        .machines
        .into_iter()
        .find(|machine| machine.machine_id == MachineId(WORKER_A.into()))
        .unwrap();
    assert_eq!(
        projected.freshness,
        protocol::ProjectionFreshness::AuthExpired
    );
}

// ---- codes --------------------------------------------------------------------

#[test]
fn codes_are_formatted_short_lived_and_stored_only_as_hmac() {
    let h = harness();
    let code = h.control.create_code().unwrap();

    let text = code.code.expose();
    assert_eq!(text.len(), 11);
    assert_eq!(&text[5..6], "-");
    assert_eq!(code.expires_at, Timestamp(START + 10 * MINUTE));
    assert_eq!(
        code.connect_command,
        "sentinal connect https://sentinal-control.example.ts.net"
    );
    assert!(!code.connect_command.contains(text));
    let stored = h.store.pairing_code(&code.code_id).unwrap().unwrap();
    assert!(!stored.code_hmac.contains(&text.replace('-', "")));
    assert_eq!(stored.code_hmac.len(), 64);
    assert_eq!(
        fs::metadata(h.dir.join("machine-credentials.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn many_codes_are_distinct() {
    let h = harness();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..5 {
        assert!(seen.insert(h.control.create_code().unwrap().code.expose().to_string()));
    }
}

#[test]
fn code_creation_is_rate_limited() {
    let h = harness();
    for _ in 0..5 {
        h.control.create_code().unwrap();
    }
    let error = h.control.create_code().unwrap_err();
    assert_eq!(
        error.failure.code,
        LinkFailureCode::RateLimited,
        "5 outstanding at most"
    );

    h.control.cancel_all_codes("test").unwrap();
    for _ in 0..5 {
        h.control.create_code().unwrap();
    }
    h.control.cancel_all_codes("test").unwrap();
    assert_eq!(
        h.control.create_code().unwrap_err().failure.code,
        LinkFailureCode::RateLimited,
        "10 per 10 minutes"
    );
    h.advance(10 * MINUTE + 1);
    assert!(h.control.create_code().is_ok());
}

#[test]
fn a_code_expires_exactly_at_its_expiry() {
    let h = harness();
    let code = h.control.create_code().unwrap();
    h.advance(10 * MINUTE - 1);
    assert_eq!(
        h.control.code_status(&code.code_id).unwrap().state,
        PairingCodeState::Waiting
    );
    h.advance(1);
    assert_eq!(
        h.control.code_status(&code.code_id).unwrap().state,
        PairingCodeState::Expired
    );
    let error = h
        .control
        .exchange(
            h.exchange_request(code.code.expose(), WORKER_A, "A"),
            "tester",
        )
        .unwrap_err();
    assert_eq!(error.failure.code, LinkFailureCode::InvalidCode);
    h.control.code_status(&code.code_id).unwrap();
    assert_eq!(
        h.audit().matches("CODE_EXPIRED").count(),
        1,
        "expiry is audited once"
    );
}

#[test]
fn a_cancelled_code_cannot_be_exchanged() {
    let h = harness();
    let code = h.control.create_code().unwrap();
    h.control.cancel_code(&code.code_id).unwrap();
    assert_eq!(
        h.control.code_status(&code.code_id).unwrap().state,
        PairingCodeState::Cancelled
    );
    assert_eq!(
        h.control
            .exchange(
                h.exchange_request(code.code.expose(), WORKER_A, "A"),
                "tester"
            )
            .unwrap_err()
            .failure
            .code,
        LinkFailureCode::InvalidCode
    );
}

#[test]
fn codes_survive_a_control_restart() {
    let h = harness();
    let code = h.control.create_code().unwrap();
    let restarted = open(&h.store, &h.clock, &h.dir);
    let response = restarted
        .exchange(
            h.exchange_request(code.code.expose(), WORKER_A, "A"),
            "tester",
        )
        .unwrap();
    assert_eq!(response.display_name, "A");
}

// ---- exchange -----------------------------------------------------------------

#[test]
fn exchange_links_a_worker_once_and_reports_connected() {
    let h = harness();
    let code = h.control.create_code().unwrap();

    let response = h
        .control
        .exchange(
            h.exchange_request(&code.code.expose().to_lowercase(), WORKER_A, "RTX PC"),
            "tester",
        )
        .unwrap();

    assert_eq!(response.control_machine_id, MachineId(CONTROL.into()));
    assert_eq!(response.worker_credential.expose().len(), 64);
    assert_eq!(response.heartbeat_interval_ms, 30_000);
    let status = h.control.code_status(&code.code_id).unwrap();
    assert_eq!(status.state, PairingCodeState::Connected);
    assert_eq!(status.machine_id, Some(MachineId(WORKER_A.into())));
    let again = h
        .control
        .exchange(
            h.exchange_request(code.code.expose(), WORKER_B, "B"),
            "tester",
        )
        .unwrap_err();
    assert_eq!(
        again.failure.code,
        LinkFailureCode::InvalidCode,
        "one use only"
    );
    assert!(
        h.secrets().contains(&"a".repeat(64)),
        "control keeps the credential it presents"
    );
    assert!(!h.secrets().contains(response.worker_credential.expose()));
}

#[test]
fn simultaneous_exchanges_of_one_code_link_exactly_one_worker() {
    let h = harness();
    let code = h.control.create_code().unwrap().code.expose().to_string();
    let control = h.control.clone();
    let results: Vec<bool> = std::thread::scope(|scope| {
        let handles: Vec<_> = [WORKER_A, WORKER_B]
            .into_iter()
            .map(|id| {
                let control = control.clone();
                let request = h.exchange_request(&code, id, id);
                scope.spawn(move || control.exchange(request, "tester").is_ok())
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    assert_eq!(results.iter().filter(|ok| **ok).count(), 1);
}

#[test]
fn invalid_worker_metadata_is_rejected_before_the_code_is_used() {
    let h = harness();
    let code = h.control.create_code().unwrap();
    let mut cases = Vec::new();
    let mut bad_id = h.exchange_request(code.code.expose(), "rtx-pc", "A");
    bad_id.machine.machine_id = MachineId("rtx-pc".into());
    cases.push(bad_id);
    cases.push(h.exchange_request(code.code.expose(), CONTROL, "A"));
    let mut bad_endpoint = h.exchange_request(code.code.expose(), WORKER_A, "A");
    bad_endpoint.endpoint = endpoint("192.168.1.5");
    cases.push(bad_endpoint);
    let mut bad_credential = h.exchange_request(code.code.expose(), WORKER_A, "A");
    bad_credential.control_credential = Secret::new("short");
    cases.push(bad_credential);
    cases.push(h.exchange_request(code.code.expose(), WORKER_A, "bad\nname"));

    for request in cases {
        let error = h.control.exchange(request, "tester").unwrap_err();
        assert_eq!(error.failure.code, LinkFailureCode::InvalidMachine);
    }
    assert_eq!(
        h.control.code_status(&code.code_id).unwrap().state,
        PairingCodeState::Waiting
    );
}

#[test]
fn twenty_wrong_codes_lock_pairing_and_cancel_outstanding_codes() {
    let h = harness();
    let real = h.control.create_code().unwrap();
    for attempt in 0..19 {
        let error = h
            .control
            .exchange(h.exchange_request("00000-00000", WORKER_A, "A"), "tester")
            .unwrap_err();
        assert_eq!(
            error.failure.code,
            LinkFailureCode::InvalidCode,
            "attempt {attempt}"
        );
    }
    let twentieth = h
        .control
        .exchange(h.exchange_request("00000-00000", WORKER_A, "A"), "tester")
        .unwrap_err();
    assert_eq!(twentieth.failure.code, LinkFailureCode::PairingLocked);
    assert_eq!(
        h.control.code_status(&real.code_id).unwrap().state,
        PairingCodeState::Cancelled
    );
    let locked = h
        .control
        .exchange(
            h.exchange_request(real.code.expose(), WORKER_A, "A"),
            "tester",
        )
        .unwrap_err();
    assert_eq!(locked.failure.code, LinkFailureCode::PairingLocked);

    h.advance(10 * MINUTE + 1);
    let fresh = h.control.create_code().unwrap();
    assert!(h
        .control
        .exchange(
            h.exchange_request(fresh.code.expose(), WORKER_A, "A"),
            "tester"
        )
        .is_ok());
}

#[test]
fn malformed_codes_count_as_wrong_without_echoing_them() {
    let h = harness();
    let error = h
        .control
        .exchange(
            h.exchange_request("SECRET-GUESS-XYZ", WORKER_A, "A"),
            "tester",
        )
        .unwrap_err();
    assert_eq!(error.failure.code, LinkFailureCode::InvalidCode);
    assert!(!error.failure.message.contains("SECRET"));
    assert_eq!(
        h.store.count_pairing_failures_since(Timestamp(0)).unwrap(),
        1
    );
}

// ---- heartbeat, refresh, revoke -------------------------------------------------

#[test]
fn heartbeats_mark_a_worker_online_then_offline_after_ninety_seconds() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    h.heartbeat(&credential, WORKER_A, 1).unwrap();

    let status = |h: &Harness| {
        h.control
            .registry()
            .unwrap()
            .machines
            .into_iter()
            .find(|machine| machine.machine_id.0 == WORKER_A)
            .unwrap()
            .status
    };
    assert_eq!(status(&h), RegistryStatus::Online);
    h.advance(90_000 - 1);
    assert_eq!(status(&h), RegistryStatus::Online);
    h.advance(1);
    assert_eq!(status(&h), RegistryStatus::Offline);
}

#[test]
fn heartbeats_reject_replays_wrong_machines_skew_and_unknown_credentials() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    h.heartbeat(&credential, WORKER_A, 1).unwrap();
    assert_eq!(
        h.heartbeat(&credential, WORKER_A, 1),
        Err(LinkFailureCode::StaleSequence)
    );
    assert_eq!(
        h.heartbeat(&credential, WORKER_B, 2),
        Err(LinkFailureCode::Unauthorized)
    );
    assert_eq!(
        h.heartbeat(&Secret::new("f".repeat(64)), WORKER_A, 3),
        Err(LinkFailureCode::Unauthorized)
    );
    let skewed = h.control.heartbeat(
        credential.expose(),
        LinkHeartbeatRequest {
            machine_id: MachineId(WORKER_A.into()),
            sequence: 4,
            sent_at: Timestamp(START - 6 * MINUTE),
            endpoint: endpoint("100.74.239.89"),
            daemon_version: "0.0.1".into(),
            capabilities: vec![],
            projection_sync: None,
            update_status: None,
            health: None,
        },
    );
    assert_eq!(
        skewed.unwrap_err().failure.code,
        LinkFailureCode::InvalidRequest
    );
}

#[test]
fn endpoint_changes_are_recorded_and_audited() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    h.control
        .heartbeat(
            credential.expose(),
            LinkHeartbeatRequest {
                machine_id: MachineId(WORKER_A.into()),
                sequence: 1,
                sent_at: h.now(),
                endpoint: endpoint("100.100.1.2"),
                daemon_version: "0.0.1".into(),
                capabilities: vec![],
                projection_sync: None,
                update_status: None,
                health: None,
            },
        )
        .unwrap();
    let entry = h
        .control
        .registry()
        .unwrap()
        .machines
        .into_iter()
        .find(|machine| machine.machine_id.0 == WORKER_A)
        .unwrap();
    assert_eq!(entry.endpoint.unwrap().address, "100.100.1.2");
    assert!(h.audit().contains("ENDPOINT_CHANGED"));
}

#[test]
fn revoking_one_worker_leaves_the_other_and_the_control_untouched() {
    let h = harness();
    let a = h.pair(WORKER_A, "A");
    let b = h.pair(WORKER_B, "B");

    h.control.revoke(&MachineId(WORKER_A.into())).unwrap();

    assert_eq!(
        h.heartbeat(&a, WORKER_A, 1),
        Err(LinkFailureCode::LinkRevoked)
    );
    assert_eq!(h.heartbeat(&b, WORKER_B, 1), Ok(()));
    let registry = h.control.registry().unwrap().machines;
    let status = |id: &str| {
        registry
            .iter()
            .find(|m| m.machine_id.0 == id)
            .unwrap()
            .status
    };
    assert_eq!(status(WORKER_A), RegistryStatus::Revoked);
    assert_eq!(status(WORKER_B), RegistryStatus::Online);
    assert_eq!(status(CONTROL), RegistryStatus::Online);
    assert!(!h.secrets().contains(&"a".repeat(64)));
    assert!(h.secrets().contains(&"b".repeat(64)), "B's credential kept");
    assert_eq!(
        h.control
            .revoke(&MachineId(CONTROL.into()))
            .unwrap_err()
            .failure
            .code,
        LinkFailureCode::InvalidRequest
    );
}

#[test]
fn rename_validates_and_never_changes_identity() {
    let h = harness();
    h.pair(WORKER_A, "RTX PC");
    h.pair(WORKER_B, "RTX PC");

    let renamed = h
        .control
        .rename(&MachineId(WORKER_A.into()), "  RTX 4090 box ")
        .unwrap();

    assert_eq!(renamed.display_name, "RTX 4090 box");
    assert_eq!(renamed.machine_id.0, WORKER_A);
    assert!(h.control.rename(&MachineId(WORKER_A.into()), "").is_err());
    assert_eq!(
        h.control
            .rename(&MachineId("01J9ZZZZZZZZZZZZZZZZZZZZZZ".into()), "x")
            .unwrap_err()
            .failure
            .code,
        LinkFailureCode::NotFound
    );
    assert!(h.audit().contains("RENAMED"));
}

#[test]
fn refresh_rotates_both_credentials_with_a_short_grace() {
    let h = harness();
    let old = h.pair(WORKER_A, "A");

    let refreshed = h
        .control
        .refresh(
            old.expose(),
            LinkRefreshRequest {
                machine_id: MachineId(WORKER_A.into()),
                control_credential: Secret::new("d".repeat(64)),
            },
        )
        .unwrap();

    assert_ne!(refreshed.worker_credential, old);
    assert!(h.secrets().contains(&"d".repeat(64)));
    assert!(!h.secrets().contains(&"c".repeat(64)));
    assert_eq!(
        h.heartbeat(&old, WORKER_A, 1),
        Ok(()),
        "old works during grace"
    );
    h.advance(60_000);
    assert_eq!(
        h.heartbeat(&old, WORKER_A, 2),
        Err(LinkFailureCode::Unauthorized)
    );
    assert_eq!(
        h.heartbeat(&refreshed.worker_credential, WORKER_A, 3),
        Ok(())
    );
}

fn refresh_request() -> LinkRefreshRequest {
    LinkRefreshRequest {
        machine_id: MachineId(WORKER_A.into()),
        control_credential: Secret::new("d".repeat(64)),
    }
}

#[test]
fn a_refresh_whose_rotation_fails_rolls_back_the_presented_credential() {
    // F5: the secrets file used to keep the new control credential even when
    // the SQLite rotation failed, so control → worker calls stopped working.
    let h = harness();
    let old = h.pair(WORKER_A, "A");
    let presented = h.secrets();
    h.control
        .inject_refresh_fault(Some(RefreshFault::RotationFails));
    assert!(h.control.refresh(old.expose(), refresh_request()).is_err());
    assert_eq!(h.secrets(), presented, "secrets must be exactly as before");
    h.advance(10 * 60_000);
    assert_eq!(
        h.heartbeat(&old, WORKER_A, 1),
        Ok(()),
        "the old credential still works"
    );
    assert!(h.audit().contains("CREDENTIAL_REFRESH_ROLLED_BACK"));
}

#[test]
fn a_crash_mid_refresh_is_rolled_back_when_the_control_restarts() {
    let h = harness();
    let old = h.pair(WORKER_A, "A");
    h.control
        .inject_refresh_fault(Some(RefreshFault::CrashBeforeCommit));
    let lost = h.control.refresh(old.expose(), refresh_request());
    assert!(
        lost.is_err(),
        "the worker never receives the new credential"
    );

    // Restart on the same store and secrets file.
    let restarted = open(&h.store, &h.clock, &h.dir);
    h.advance(10 * 60_000);
    let heartbeat = restarted.heartbeat(
        old.expose(),
        LinkHeartbeatRequest {
            machine_id: MachineId(WORKER_A.into()),
            sequence: 1,
            sent_at: h.now(),
            endpoint: endpoint("100.74.239.89"),
            daemon_version: "0.0.1".into(),
            capabilities: vec![MachineCapability::Sessions],
            projection_sync: None,
            update_status: None,
            health: None,
        },
    );
    assert!(
        heartbeat.is_ok(),
        "the worker's credential must survive the crash"
    );
    assert!(
        !h.secrets().contains(&"d".repeat(64)),
        "the unreceived control credential is gone"
    );
    assert!(!h.secrets().contains("pending_refresh"));
    assert!(h.audit().contains("CREDENTIAL_REFRESH_ROLLED_BACK"));
}

#[test]
fn a_worker_still_on_its_previous_credential_is_told_to_refresh_again() {
    // A refresh reply lost in transit leaves the worker on its old credential;
    // during the grace period it must be asked to refresh, not locked out.
    let h = harness();
    let old = h.pair(WORKER_A, "A");
    let refreshed = h.control.refresh(old.expose(), refresh_request()).unwrap();
    let ask = |credential: &Secret, sequence| {
        h.control
            .heartbeat(
                credential.expose(),
                LinkHeartbeatRequest {
                    machine_id: MachineId(WORKER_A.into()),
                    sequence,
                    sent_at: h.now(),
                    endpoint: endpoint("100.74.239.89"),
                    daemon_version: "0.0.1".into(),
                    capabilities: vec![MachineCapability::Sessions],
                    projection_sync: None,
                    update_status: None,
                    health: None,
                },
            )
            .unwrap()
            .refresh_due
    };
    assert!(ask(&old, 1), "previous credential → refresh again");
    assert!(
        !ask(&refreshed.worker_credential, 2),
        "current credential → nothing to do"
    );
}

#[test]
fn worker_disconnect_marks_it_disconnected() {
    let h = harness();
    let credential = h.pair(WORKER_A, "A");
    h.control
        .disconnect(credential.expose(), &MachineId(WORKER_A.into()))
        .unwrap();
    let entry = h.control.registry().unwrap().machines;
    assert_eq!(
        entry
            .iter()
            .find(|m| m.machine_id.0 == WORKER_A)
            .unwrap()
            .status,
        RegistryStatus::Disconnected
    );
    assert_eq!(
        h.heartbeat(&credential, WORKER_A, 1),
        Err(LinkFailureCode::LinkRevoked)
    );
}

#[test]
fn password_change_cancels_outstanding_codes() {
    let h = harness();
    let code = h.control.create_code().unwrap();
    assert_eq!(h.control.cancel_all_codes("PASSWORD_CHANGED").unwrap(), 1);
    assert_eq!(
        h.control.code_status(&code.code_id).unwrap().state,
        PairingCodeState::Cancelled
    );
}

#[test]
fn registry_and_audit_never_contain_secrets() {
    let h = harness();
    let code = h.control.create_code().unwrap();
    let response = h
        .control
        .exchange(
            h.exchange_request(code.code.expose(), WORKER_A, "A"),
            "tester",
        )
        .unwrap();
    let _ = h
        .control
        .exchange(h.exchange_request("ABCDE-FGHJK", WORKER_B, "B"), "tester");
    let registry = serde_json::to_string(&h.control.registry().unwrap()).unwrap();
    let audit = h.audit();
    for secret in [
        code.code.expose().to_string(),
        code.code.expose().replace('-', ""),
        "ABCDEFGHJK".to_string(),
        "c".repeat(64),
        response.worker_credential.expose().to_string(),
    ] {
        assert!(!registry.contains(&secret), "registry leaks a secret");
        assert!(!audit.contains(&secret), "audit leaks a secret");
    }
    for kind in ["CODE_CREATED", "EXCHANGE_SUCCEEDED", "EXCHANGE_FAILED"] {
        assert!(audit.contains(kind), "{kind} missing from {audit}");
    }
    assert_eq!(
        fs::metadata(h.dir.join("pairing-audit.jsonl"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

fn health_heartbeat(
    machine_id: &str,
    sequence: u64,
    at: i64,
    restarts: u32,
) -> LinkHeartbeatRequest {
    LinkHeartbeatRequest {
        machine_id: MachineId(machine_id.into()),
        sequence,
        sent_at: protocol::Timestamp(at),
        endpoint: endpoint("100.74.239.89"),
        daemon_version: "0.3.0".into(),
        capabilities: vec![MachineCapability::Sessions],
        projection_sync: None,
        update_status: None,
        health: Some(protocol::observability::DaemonHealthReport {
            at: protocol::Timestamp(at),
            rss_bytes: 20_000_000,
            cpu_percent: 0.5,
            live_sessions: 2,
            log_disk_bytes: 4_096,
            uptime_ms: 60_000,
            restarts,
            upload_delay_ms: Some(900),
            upload_failures: 0,
            errors: vec![],
        }),
    }
}

#[test]
fn worker_health_is_stored_under_the_authenticated_machine_and_restarts_are_noted() {
    use protocol::observability::{HealthMetric, IncidentKind};
    let h = harness();
    let a = h.pair(WORKER_A, "A");
    let b = h.pair(WORKER_B, "B");
    let now = h.now().0;

    // B's credential cannot report health (or anything) as A.
    assert!(h
        .control
        .heartbeat(b.expose(), health_heartbeat(WORKER_A, 1, now, 1))
        .is_err());
    let none = h
        .store
        .health_stats(protocol::Timestamp(0), protocol::Timestamp(i64::MAX))
        .unwrap();
    assert!(none.is_empty(), "nothing stored for a refused heartbeat");

    h.control
        .heartbeat(a.expose(), health_heartbeat(WORKER_A, 1, now, 4))
        .unwrap();
    h.control
        .heartbeat(a.expose(), health_heartbeat(WORKER_A, 2, now + 1, 4))
        .unwrap();
    let rows = h
        .store
        .health_stats(protocol::Timestamp(0), protocol::Timestamp(i64::MAX))
        .unwrap();
    assert!(rows.iter().all(|row| row.machine_id.0 == WORKER_A));
    let rss = rows
        .iter()
        .find(|row| row.metric == HealthMetric::DaemonRssBytes)
        .unwrap();
    assert_eq!(rss.stat.count, 2);
    assert!(
        h.store
            .health_incidents(protocol::Timestamp(0))
            .unwrap()
            .is_empty(),
        "same start count: no restart"
    );

    h.control
        .heartbeat(a.expose(), health_heartbeat(WORKER_A, 3, now + 2, 5))
        .unwrap();
    let incidents = h.store.health_incidents(protocol::Timestamp(0)).unwrap();
    assert_eq!(incidents.len(), 1);
    assert_eq!(incidents[0].kind, IncidentKind::Restarted);
    assert_eq!(incidents[0].detail.as_deref(), Some("0.3.0"));
}

fn open_incidents(h: &Harness) -> Vec<(String, protocol::observability::IncidentKind)> {
    h.store
        .health_incidents(protocol::Timestamp(0))
        .unwrap()
        .into_iter()
        .filter(|incident| incident.ended_at.is_none())
        .map(|incident| (incident.machine_id.0, incident.kind))
        .collect()
}

#[test]
fn freshness_opens_and_closes_offline_and_delayed_incidents() {
    use protocol::observability::IncidentKind;
    let h = harness();
    let a = h.pair(WORKER_A, "A");
    h.heartbeat(&a, WORKER_A, 1).unwrap();
    h.control.observe_freshness();
    assert!(
        open_incidents(&h).is_empty(),
        "online without projection is not delayed"
    );

    h.control
        .accept_projection(a.expose(), projection_batch(WORKER_A))
        .unwrap();
    h.control.observe_freshness();
    assert!(open_incidents(&h).is_empty(), "just synced: live");

    h.advance(11_000);
    h.control.observe_freshness();
    assert_eq!(
        open_incidents(&h),
        vec![(WORKER_A.into(), IncidentKind::Delayed)]
    );

    h.advance(90_000);
    h.control.observe_freshness();
    assert_eq!(
        open_incidents(&h),
        vec![(WORKER_A.into(), IncidentKind::Offline)],
        "offline replaces delayed"
    );

    h.heartbeat(&a, WORKER_A, 2).unwrap();
    h.control.observe_freshness();
    assert_eq!(
        open_incidents(&h),
        vec![(WORKER_A.into(), IncidentKind::Delayed)],
        "back, projection still old"
    );
    let ended = h.store.health_incidents(protocol::Timestamp(0)).unwrap();
    assert!(ended
        .iter()
        .any(|incident| incident.kind == IncidentKind::Offline && incident.ended_at.is_some()));
}

#[test]
fn only_online_unrevoked_workers_are_sampled_and_relays_are_incidents() {
    use protocol::{link::TailnetPath, observability::IncidentKind};
    let h = harness();
    let a = h.pair(WORKER_A, "A");
    let b = h.pair(WORKER_B, "B");
    h.heartbeat(&a, WORKER_A, 1).unwrap();
    h.heartbeat(&b, WORKER_B, 1).unwrap();
    h.control.revoke(&MachineId(WORKER_B.into())).unwrap();
    assert_eq!(
        h.control.round_trip_targets(),
        vec![MachineId(WORKER_A.into())]
    );
    h.advance(91_000);
    assert!(
        h.control.round_trip_targets().is_empty(),
        "offline is not sampled"
    );

    let id = MachineId(WORKER_A.into());
    h.control
        .record_tailnet_path(&id, TailnetPath::Relayed, Some("fra"));
    h.control
        .record_tailnet_path(&id, TailnetPath::Unknown, None);
    assert_eq!(
        open_incidents(&h),
        vec![(WORKER_A.into(), IncidentKind::Relayed)]
    );
    h.control
        .record_tailnet_path(&id, TailnetPath::Direct, None);
    assert!(open_incidents(&h).is_empty());
}

#[tokio::test]
async fn the_monitor_stops_on_shutdown() {
    let h = harness();
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let task = daemon::control::health_monitor::spawn(h.control.clone(), stopped);
    stop.send(true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .expect("monitor exits after shutdown")
        .unwrap();
}
