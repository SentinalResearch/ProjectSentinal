//! Real `sentinald` adopting a real same-user child process read-only.
//!
//! Run with: `cargo build -p daemon --bin sentinald && cargo test -p cli --test
//! adopt_process_integration -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use cli::client::DaemonClient;
use futures_util::StreamExt;
use protocol::{
    CaptureLevel, ClientRequest, DaemonMessage, Envelope, LogSource, Session, SessionOrigin,
    SessionState,
};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
use support::{short_dir, wait_until, Daemon};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

fn sentinal() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_sentinal"))
}

async fn sessions(daemon: &Daemon) -> Vec<Session> {
    let mut client = DaemonClient::connect(std::slice::from_ref(&daemon.url), &daemon.token)
        .await
        .unwrap();
    match client.request(ClientRequest::ListSessions).await.unwrap() {
        DaemonMessage::Sessions { sessions } => sessions,
        other => panic!("unexpected {other:?}"),
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

fn adopt(daemon: &Daemon, args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new(sentinal())
        .arg("adopt")
        .args(args)
        .env("SENTINAL_SOCKET", &daemon.socket)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "starts a real sentinald and adopts a real process"]
async fn adopt_process_integration() {
    let daemon = Daemon::start(sentinal());
    let dir = short_dir("adp");
    let log = dir.join("job.log");
    let log_file = std::fs::File::create(&log).unwrap();
    let mut job = Command::new("sh")
        .args([
            "-c",
            "i=0; while [ $i -lt 600 ]; do echo tick $i; i=$((i+1)); sleep 0.1; done",
        ])
        .stdin(Stdio::null())
        .stdout(log_file.try_clone().unwrap())
        .stderr(log_file)
        .spawn()
        .unwrap();
    let pid = job.id();

    // --pick lists the job and adopts nothing on cancel.
    let picked = tokio::task::block_in_place(|| adopt(&daemon, &["--pick"], "\n"));
    let listing = String::from_utf8_lossy(&picked.stdout);
    assert!(listing.contains(&pid.to_string()), "{listing}");
    assert!(listing.contains("Nothing adopted."), "{listing}");
    assert!(sessions(&daemon).await.is_empty());

    // Adopt by PID.
    let output = tokio::task::block_in_place(|| {
        adopt(&daemon, &[&pid.to_string(), "-t", "Integration job"], "")
    });
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("[ADOPTED · PROCESS]") && text.contains("exit code will be unknown"),
        "{text}"
    );
    let duplicate = tokio::task::block_in_place(|| adopt(&daemon, &[&pid.to_string()], ""));
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already adopted"));

    let session = sessions(&daemon)
        .await
        .into_iter()
        .find(|session| session.pid == Some(pid))
        .unwrap();
    // The reply is PROCESS; background discovery may already have upgraded it.
    assert_eq!(session.origin, SessionOrigin::Adopted);
    assert!(
        matches!(
            session.capture,
            CaptureLevel::Process | CaptureLevel::ProcessLog
        ),
        "{:?}",
        session.capture
    );
    assert_eq!(session.state, SessionState::Running);
    assert_eq!(session.title.as_deref(), Some("Integration job"));

    // Redirected stdout/stderr are discovered off the request path.
    let expected_log = log.canonicalize().unwrap();
    let mut discovered = None;
    tokio::task::block_in_place(|| {
        wait_until("log discovery", || {
            let handle = tokio::runtime::Handle::current();
            discovered = handle
                .block_on(sessions(&daemon))
                .into_iter()
                .find(|s| s.id == session.id)
                .and_then(|s| s.log);
            discovered.is_some()
        });
    });
    let discovered = discovered.unwrap();
    // A readable discovered log is tailed (1b.D), so capture becomes PROCESS_LOG.
    tokio::task::block_in_place(|| {
        wait_until("PROCESS_LOG capture", || {
            let handle = tokio::runtime::Handle::current();
            handle
                .block_on(sessions(&daemon))
                .into_iter()
                .any(|s| s.id == session.id && s.capture == CaptureLevel::ProcessLog)
        });
    });
    assert_eq!(
        Path::new(&discovered.path).canonicalize().unwrap(),
        expected_log
    );
    assert_eq!(discovered.source, LogSource::StdoutAndStderr);

    // Live Metrics reach a client that never attaches.
    let mut request = daemon.url.as_str().into_client_request().unwrap();
    let token = std::fs::read_to_string(&daemon.token).unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", token.trim())).unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let metrics = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Text(text) = message {
                if let DaemonMessage::SessionMetrics { metrics } =
                    Envelope::<DaemonMessage>::decode(text.as_str())
                        .unwrap()
                        .payload
                {
                    if metrics.session_id == session.id {
                        return metrics;
                    }
                }
            }
        }
        panic!("socket closed before metrics arrived");
    })
    .await
    .expect("no live SESSION_METRICS for the adopted process within 10 s");
    assert!(metrics.totals.rss_bytes > 0);

    // Read-only: a remote kill is refused and the process keeps running.
    let mut client = DaemonClient::connect(std::slice::from_ref(&daemon.url), &daemon.token)
        .await
        .unwrap();
    let refused = client
        .request(ClientRequest::Kill {
            session_id: session.id,
        })
        .await
        .unwrap();
    assert!(matches!(&refused, DaemonMessage::Error { message } if message.contains("PROCESS")));
    assert!(alive(pid));

    // The test (not Sentinal) ends the job; the Session ends with unknown exit.
    job.kill().unwrap();
    job.wait().unwrap();
    tokio::task::block_in_place(|| {
        wait_until("adopted Session to end", || {
            tokio::runtime::Handle::current()
                .block_on(sessions(&daemon))
                .into_iter()
                .any(|s| s.id == session.id && s.state == SessionState::Completed)
        });
    });
    let ended = sessions(&daemon)
        .await
        .into_iter()
        .find(|s| s.id == session.id)
        .unwrap();
    assert_eq!(ended.exit_code, None);
    let history = match client
        .request(ClientRequest::GetHistory {
            session_id: Some(session.id),
            limit: 10,
        })
        .await
        .unwrap()
    {
        DaemonMessage::History { commands } => commands,
        other => panic!("unexpected {other:?}"),
    };
    assert!(
        history.is_empty(),
        "no Command is fabricated for an adopted process"
    );
    daemon.stop();
    eprintln!(
        "adopt: pid {pid} adopted as Session {}, log {} ({:?}), metrics rss {} B, ended with unknown exit",
        session.id.0, discovered.path, discovered.source, metrics.totals.rss_bytes
    );
}
