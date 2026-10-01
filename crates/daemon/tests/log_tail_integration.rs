//! A real process writes, truncates, and rotates its log while adopted.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    process_probe::ProcessProbe,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{CaptureLevel, EventKind, MachineId, SessionState, Timestamp};
use std::{os::unix::fs::MetadataExt, process::Command, sync::Arc, time::Duration};
use store::Store;

/// `$0`/`$1` make the argv read `... python train.py`, selecting the training adapter.
const WRITER: &str = r#"
log=$2
i=1
while [ $i -le 3 ]; do echo "Epoch $i/9 loss=0.5" >> "$log"; sleep 0.2; i=$((i+1)); done
: > "$log"
echo "Epoch 4/9 loss=0.4" >> "$log"; sleep 0.3
mv "$log" "$log.1"
echo "Epoch 5/9 loss=0.3" > "$log"; sleep 0.3
echo "Epoch 6/9 loss=0.2" >> "$log"; sleep 0.3
printf "Epoch 7/9 loss=0.1" >> "$log"
"#;

fn now() -> Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Timestamp(i64::try_from(millis).unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "runs a real writer process and polls a real log"]
async fn log_tail_integration() {
    let data_dir = std::env::temp_dir().join(format!("sentinal-tail-int-{}", std::process::id()));
    std::fs::create_dir_all(&data_dir).unwrap();
    let log = data_dir.join("train.log");
    std::fs::write(&log, "").unwrap();
    let uid = std::fs::metadata(&log).unwrap().uid();
    let store = Arc::new(Store::open(&data_dir.join("s.sqlite3")).unwrap());
    let manager = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("integration".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(now),
        },
        AuditLog::new(&data_dir),
        store.clone(),
    )
    .unwrap()
    .with_process_probe(ProcessProbe::system(uid));

    let mut child = Command::new("/bin/sh")
        .args(["-c", WRITER, "python", "train.py"])
        .arg(&log)
        .spawn()
        .unwrap();
    let pid = child.id();
    // Reap the writer so its exit is visible instead of lingering as a zombie.
    let reaper = std::thread::spawn(move || child.wait().unwrap());

    let session = manager
        .adopt_process(pid, None, Some(log.display().to_string()))
        .unwrap();
    assert_eq!(session.capture, CaptureLevel::ProcessLog);

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            manager.check_external_processes().unwrap();
            if manager.session(session.id).unwrap().state.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert!(reaper.join().unwrap().success());

    let mut epochs: Vec<u64> = store
        .events(Some(session.id), None, 200)
        .unwrap()
        .into_iter()
        .filter_map(|event| match event.kind {
            EventKind::ProgressUpdated { current, .. } => Some(current),
            _ => None,
        })
        .collect();
    let ended = manager.session(session.id).unwrap();
    epochs.sort_unstable();
    assert_eq!(epochs, vec![1, 2, 3, 4, 5, 6, 7], "each epoch exactly once");
    assert_eq!(
        (ended.state, ended.exit_code),
        (SessionState::Completed, None)
    );
    assert!(store.history(Some(session.id), 10).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(data_dir);
}
