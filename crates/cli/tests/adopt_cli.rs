//! `sentinal adopt`: ps parsing, candidate selection, and socket behavior.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::adopt_command::{candidates, choose, describe, parse_ps, Choice, PsRow};
use protocol::{
    CaptureLevel, Envelope, LocalErrorCode, LocalRequest, LocalResponse, LogSource, MachineId,
    Session, SessionId, SessionLog, SessionOrigin, SessionState, Timestamp,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

const PS: &str = "\
    1     0     0   10-01:00:00 /sbin/launchd
  500     1   501      01:00:00 /bin/zsh -l
  600   500   501         05:00 sentinal adopt --pick
  601   600   501         00:00 ps -A -o pid=,ppid=,uid=,etime=,args=
  700     1   501      02:00:00 /usr/local/bin/sentinald
  800   500   501      00:30:00 python train.py --config moe.yaml
  900     1   502      00:10:00 /usr/bin/other-user-job
  950   500   501         00:02 sleep 1000
 garbage line
";

fn rows() -> Vec<PsRow> {
    parse_ps(PS)
}

#[test]
fn ps_rows_parse_and_skip_garbage() {
    let rows = rows();
    assert_eq!(rows.len(), 8);
    assert_eq!(rows[5].pid, 800);
    assert_eq!(rows[5].args, "python train.py --config moe.yaml");
    assert_eq!(rows[5].elapsed, "00:30:00");
}

#[test]
fn candidates_are_same_user_and_exclude_self_parent_ps_and_sentinald() {
    let pids: Vec<u32> = candidates(&rows(), 600).iter().map(|row| row.pid).collect();
    assert_eq!(pids, vec![950, 800]);
    assert!(
        candidates(&rows(), 12345).is_empty(),
        "unknown self lists nothing"
    );
}

#[test]
fn choices_accept_list_numbers_and_listed_pids_only() {
    let listed = candidates(&rows(), 600);
    assert_eq!(choose(&listed, "1\n"), Choice::Adopt(950));
    assert_eq!(choose(&listed, " 2 "), Choice::Adopt(800));
    assert_eq!(choose(&listed, "800"), Choice::Adopt(800));
    assert_eq!(choose(&listed, ""), Choice::Cancel);
    assert_eq!(choose(&listed, "q"), Choice::Cancel);
    assert!(
        matches!(choose(&listed, "900"), Choice::Invalid(_)),
        "other user's pid"
    );
    assert!(matches!(choose(&listed, "3"), Choice::Invalid(_)));
    assert!(matches!(choose(&listed, "abc"), Choice::Invalid(_)));
}

fn adopted(log: Option<SessionLog>) -> Session {
    let mut session = Session::new(
        SessionId(7),
        MachineId("m".into()),
        "/usr/bin/python3",
        "/work",
        Timestamp(1),
    );
    session.origin = SessionOrigin::Adopted;
    session.capture = CaptureLevel::Process;
    session.state = SessionState::Running;
    session.pid = Some(800);
    session.title = Some("python3".into());
    session.log = log;
    session
}

#[test]
fn the_summary_is_honest_about_capture_and_exit_code() {
    let text = describe(800, &adopted(None));
    assert!(text.contains("Session 7 [ADOPTED · PROCESS]"));
    assert!(text.contains("exit code will be unknown"));
    let with_log = describe(
        800,
        &adopted(Some(SessionLog {
            path: "/work/train.log".into(),
            source: LogSource::Explicit,
        })),
    );
    assert!(with_log.contains("tailing not enabled yet"));
}

// ---- the real binary against a fake local socket ---------------------------

static NEXT: AtomicU64 = AtomicU64::new(1);

fn short_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sad{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Fake {
    socket: PathBuf,
    connections: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<LocalRequest>>>,
}

fn fake(reply: LocalResponse) -> Fake {
    let socket = short_dir().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (count, log) = (connections.clone(), requests.clone());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let mut header = [0_u8; 4];
            stream.read_exact(&mut header).await.unwrap();
            let mut body = vec![0; u32::from_be_bytes(header) as usize];
            stream.read_exact(&mut body).await.unwrap();
            log.lock().unwrap().push(
                Envelope::<LocalRequest>::decode(std::str::from_utf8(&body).unwrap())
                    .unwrap()
                    .payload,
            );
            let text = Envelope::new(reply.clone()).encode().unwrap();
            stream
                .write_all(&u32::try_from(text.len()).unwrap().to_be_bytes())
                .await
                .unwrap();
            stream.write_all(text.as_bytes()).await.unwrap();
        }
    });
    Fake {
        socket,
        connections,
        requests,
    }
}

async fn adopt(socket: &Path, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .arg("adopt")
        .args(args)
        .env("SENTINAL_SOCKET", socket)
        .env("HOME", short_dir())
        .output()
        .await
        .unwrap()
}

#[tokio::test]
async fn adopting_sends_one_local_request_and_prints_the_summary() {
    let fake = fake(LocalResponse::Session {
        session: adopted(None),
    });
    let output = adopt(&fake.socket, &["800", "-t", "Qwen"]).await;
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[ADOPTED · PROCESS]"), "{stdout}");
    assert_eq!(
        *fake.requests.lock().unwrap(),
        vec![LocalRequest::AdoptProcess {
            pid: 800,
            title: Some("Qwen".into()),
            log_path: None,
        }]
    );
}

#[tokio::test]
async fn a_relative_log_is_sent_as_an_absolute_path() {
    let fake = fake(LocalResponse::Session {
        session: adopted(None),
    });
    let dir = short_dir();
    std::fs::write(dir.join("train.log"), "x").unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args(["adopt", "800", "--log", "train.log"])
        .current_dir(&dir)
        .env("SENTINAL_SOCKET", &fake.socket)
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let requests = fake.requests.lock().unwrap().clone();
    assert!(matches!(
        &requests[0],
        LocalRequest::AdoptProcess { log_path: Some(path), .. } if path.starts_with('/') && path.ends_with("/train.log")
    ));
}

#[tokio::test]
async fn daemon_refusals_are_clear_errors() {
    let fake = fake(LocalResponse::Error {
        code: LocalErrorCode::Conflict,
        message: "process 800 is already adopted as Session 3".into(),
    });
    let output = adopt(&fake.socket, &["800"]).await;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refused to adopt pid 800: process 800 is already adopted"),
        "{stderr}"
    );
}

#[tokio::test]
async fn an_unreachable_daemon_adopts_nothing() {
    let output = adopt(&short_dir().join("absent.sock"), &["800"]).await;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot reach sentinald") && stderr.contains("nothing was adopted"),
        "{stderr}"
    );
}

#[tokio::test]
async fn a_bad_log_or_missing_target_is_rejected_before_contact() {
    let fake = fake(LocalResponse::Session {
        session: adopted(None),
    });
    let dir = short_dir();
    let output = adopt(&fake.socket, &["800", "--log", &dir.display().to_string()]).await;
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a regular file"));

    let output = adopt(&fake.socket, &[]).await;
    assert!(!output.status.success(), "a PID or --pick is required");
    assert_eq!(fake.connections.load(Ordering::SeqCst), 0);
}
