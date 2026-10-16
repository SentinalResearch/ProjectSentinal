//! `sentinal setup` waiting for approval, against a fake local daemon.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::approval_command::{approve, ApprovalTarget};
use protocol::{
    link::{ConnectionReport, LocalLinkState},
    pairing::PairingRequestState,
    Envelope, LocalRequest, LocalResponse, MachineId, Timestamp,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

const CONTROL: &str = "01K2C7RQ9M3X8V6T4N2P0SDB5A";
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fake {
    socket: PathBuf,
    requests: Arc<Mutex<Vec<LocalRequest>>>,
}

fn fake_daemon(mut replies: Vec<LocalResponse>) -> Fake {
    let dir = std::env::temp_dir().join(format!(
        "sap{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    replies.reverse();
    let replies = Arc::new(Mutex::new(replies));
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut header = [0_u8; 4];
            stream.read_exact(&mut header).await.unwrap();
            let mut body = vec![0; u32::from_be_bytes(header) as usize];
            stream.read_exact(&mut body).await.unwrap();
            log.lock().unwrap().push(
                Envelope::<LocalRequest>::decode(std::str::from_utf8(&body).unwrap())
                    .unwrap()
                    .payload,
            );
            let reply = replies.lock().unwrap().pop().expect("unexpected request");
            let text = Envelope::new(reply).encode().unwrap();
            stream
                .write_all(&u32::try_from(text.len()).unwrap().to_be_bytes())
                .await
                .unwrap();
            stream.write_all(text.as_bytes()).await.unwrap();
        }
    });
    Fake { socket, requests }
}

fn requested() -> LocalResponse {
    LocalResponse::ApprovalRequested {
        request_id: "pr_9f2c1a0b7d3e4f51".into(),
        verification_words: vec![
            "squid".into(),
            "thistle".into(),
            "otter".into(),
            "arrow".into(),
        ],
        expires_at: Timestamp(1_791_900_600_000),
    }
}

fn state(state: PairingRequestState) -> LocalResponse {
    LocalResponse::ApprovalState { state }
}

async fn run(fake: &Fake, cancel: impl std::future::Future<Output = ()>) -> (i32, String, String) {
    let control_machine_id = MachineId(CONTROL.into());
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = approve(
        &fake.socket,
        ApprovalTarget {
            control_url: "https://pilot.example.ts.net",
            control_name: "Owner's Sentinal",
            control_machine_id: &control_machine_id,
            keep_current_link: false,
        },
        Some("RTX PC"),
        Duration::from_millis(5),
        cancel,
        &mut out,
        &mut err,
    )
    .await;
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[tokio::test]
async fn shows_the_words_then_polls_until_connected() {
    let fake = fake_daemon(vec![
        requested(),
        state(PairingRequestState::Pending),
        state(PairingRequestState::Pending),
        LocalResponse::Connection {
            connection: ConnectionReport {
                state: LocalLinkState::Active,
                machine_id: MachineId("01J9Z3K8V6Q2T7M4X5C0RNB8DA".into()),
                control_url: Some("https://pilot.example.ts.net".into()),
                control_machine_id: Some(MachineId(CONTROL.into())),
                display_name: Some("Lab RTX".into()),
                link_id: Some("lk_1".into()),
                paired_at: None,
                last_heartbeat_at: None,
                message: None,
            },
        },
    ]);
    let (code, stdout, stderr) = run(&fake, std::future::pending()).await;
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("squid · thistle · otter · arrow"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Owner's Sentinal (control 01K2C7RQ)"),
        "{stdout}"
    );
    assert!(stdout.contains("Connected as \"Lab RTX\""), "{stdout}");
    let requests = fake.requests.lock().unwrap();
    assert!(matches!(
        &requests[0],
        LocalRequest::RequestApproval { control_machine_id, display_name: Some(name), .. }
            if control_machine_id.0 == CONTROL && name == "RTX PC"
    ));
    assert_eq!(requests.len(), 4);
    assert!(requests[1..]
        .iter()
        .all(|request| *request == LocalRequest::ClaimApproval));
}

#[tokio::test]
async fn ctrl_c_withdraws_the_request() {
    let fake = fake_daemon(vec![requested(), state(PairingRequestState::Cancelled)]);
    let (code, _, stderr) = run(&fake, async {}).await;
    assert_eq!(code, 2);
    assert!(stderr.contains("withdrawn"), "{stderr}");
    assert_eq!(
        fake.requests.lock().unwrap()[1],
        LocalRequest::CancelApproval
    );
}

#[tokio::test]
async fn a_rejection_or_expiry_ends_with_exit_4() {
    for (ended, message) in [
        (PairingRequestState::Rejected, "rejected"),
        (PairingRequestState::Expired, "expired"),
    ] {
        let fake = fake_daemon(vec![requested(), state(ended)]);
        let (code, _, stderr) = run(&fake, std::future::pending()).await;
        assert_eq!(code, 4);
        assert!(stderr.contains(message), "{stderr}");
    }
}
