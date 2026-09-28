//! Private local-ingress behavior and filesystem safety tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::local_socket::{
    current_uid, handle_connection, path::inspect_existing_socket, SecurityAudit, UnixSocketServer,
    MAX_ERROR_MESSAGE_BYTES,
};
use protocol::{Envelope, LocalErrorCode, LocalRequest, LocalResponse, MAX_LOCAL_FRAME_BYTES};
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        // Short names keep socket paths under the macOS 104-byte sun_path limit.
        "sl{}-{}-{name}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

async fn exchange(
    peer_uid: Option<u32>,
    expected_uid: u32,
    body: &[u8],
    declared_length: usize,
    audit: SecurityAudit,
    socket_path: PathBuf,
) -> Envelope<LocalResponse> {
    let (mut client, server) = tokio::net::UnixStream::pair().unwrap();
    let task = tokio::spawn(async move {
        handle_connection(server, expected_uid, peer_uid, audit, &socket_path).await
    });
    client
        .write_all(&u32::try_from(declared_length).unwrap().to_be_bytes())
        .await
        .unwrap();
    if !body.is_empty() {
        client.write_all(body).await.unwrap();
    }
    let mut length = [0_u8; 4];
    client.read_exact(&mut length).await.unwrap();
    let length = u32::from_be_bytes(length) as usize;
    let mut response = vec![0; length];
    client.read_exact(&mut response).await.unwrap();
    task.await.unwrap().unwrap();
    Envelope::decode(std::str::from_utf8(&response).unwrap()).unwrap()
}

#[tokio::test]
async fn wrong_uid_is_rejected_before_malformed_json_is_decoded_and_audited_without_payload() {
    let data_dir = temp_dir("wrong-uid");
    let audit = SecurityAudit::new(&data_dir);
    let response = exchange(
        Some(501),
        502,
        b"password=hunter2{not-json",
        25,
        audit,
        data_dir.join("run/sentineld.sock"),
    )
    .await;

    assert!(matches!(
        response.payload,
        LocalResponse::Error {
            code: LocalErrorCode::Forbidden,
            ..
        }
    ));
    let log = fs::read_to_string(data_dir.join("security-audit.jsonl")).unwrap();
    assert!(log.contains("WRONG_PEER_UID"));
    assert!(!log.contains("hunter2"));
    if data_dir.exists() {
        fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn valid_peer_gets_bounded_errors_for_malformed_and_oversized_frames() {
    let data_dir = temp_dir("bounded");
    let audit = SecurityAudit::new(&data_dir);
    let malformed = exchange(
        Some(501),
        501,
        b"{bad",
        4,
        audit.clone(),
        data_dir.join("run/sentineld.sock"),
    )
    .await;
    let oversized = exchange(
        Some(501),
        501,
        &[],
        MAX_LOCAL_FRAME_BYTES + 1,
        audit,
        data_dir.join("run/sentineld.sock"),
    )
    .await;

    for response in [malformed, oversized] {
        let LocalResponse::Error { code, message } = response.payload else {
            panic!("expected a local error")
        };
        assert_eq!(code, LocalErrorCode::InvalidRequest);
        assert!(message.len() <= MAX_ERROR_MESSAGE_BYTES);
    }
    if data_dir.exists() {
        fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn valid_but_unimplemented_local_operation_cannot_send_pty_input() {
    let data_dir = temp_dir("unavailable");
    let request = Envelope::new(LocalRequest::AdoptProcess {
        pid: 4242,
        title: None,
        log_path: None,
    })
    .encode()
    .unwrap();
    let response = exchange(
        Some(501),
        501,
        request.as_bytes(),
        request.len(),
        SecurityAudit::new(&data_dir),
        data_dir.join("run/sentineld.sock"),
    )
    .await;

    assert!(matches!(
        response.payload,
        LocalResponse::Error {
            code: LocalErrorCode::Unavailable,
            ..
        }
    ));
    assert!(!data_dir.join("audit.jsonl").exists());
}

#[tokio::test]
async fn listener_creates_private_parent_and_socket_and_removes_only_its_instance() {
    let data_dir = temp_dir("modes");
    fs::create_dir_all(&data_dir).unwrap();
    let socket_path = data_dir.join("run/sentineld.sock");
    let server = UnixSocketServer::bind(&socket_path, SecurityAudit::new(&data_dir)).unwrap();

    let parent_mode = fs::metadata(socket_path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let socket = fs::symlink_metadata(&socket_path).unwrap();
    assert_eq!(parent_mode, 0o700);
    assert_eq!(socket.permissions().mode() & 0o777, 0o600);
    assert!(socket.file_type().is_socket());

    drop(server);
    assert!(!socket_path.exists());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn listener_removes_safe_stale_socket_but_refuses_symlink() {
    let data_dir = temp_dir("stale");
    fs::create_dir_all(data_dir.join("run")).unwrap();
    let socket_path = data_dir.join("run/sentineld.sock");
    let stale = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
    drop(stale);

    let server = UnixSocketServer::bind(&socket_path, SecurityAudit::new(&data_dir)).unwrap();
    drop(server);

    let target = data_dir.join("target");
    fs::write(&target, b"do not remove").unwrap();
    std::os::unix::fs::symlink(&target, &socket_path).unwrap();
    let error = UnixSocketServer::bind(&socket_path, SecurityAudit::new(&data_dir)).unwrap_err();
    assert!(error.to_string().contains("symlink"));
    assert_eq!(fs::read(&target).unwrap(), b"do not remove");
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn current_uid_matches_the_owner_of_files_this_process_creates() {
    let data_dir = temp_dir("uid");
    fs::create_dir_all(&data_dir).unwrap();
    let owner = fs::metadata(&data_dir).unwrap().uid();
    assert_eq!(current_uid(&data_dir).unwrap(), owner);
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn listener_refuses_parent_owned_by_another_uid() {
    let data_dir = temp_dir("fp");
    fs::create_dir_all(data_dir.join("run")).unwrap();
    let socket_path = data_dir.join("run/sentineld.sock");
    let other_uid = current_uid(&data_dir).unwrap().wrapping_add(1);

    let error =
        UnixSocketServer::bind_for_uid(&socket_path, SecurityAudit::new(&data_dir), other_uid)
            .unwrap_err();
    assert!(error.to_string().contains("parent uid"), "{error}");
    assert!(!socket_path.exists());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn stale_socket_owned_by_another_uid_is_refused_and_kept() {
    let data_dir = temp_dir("fs");
    fs::create_dir_all(data_dir.join("run")).unwrap();
    let socket_path = data_dir.join("run/sentineld.sock");
    drop(std::os::unix::net::UnixListener::bind(&socket_path).unwrap());
    let other_uid = current_uid(&data_dir).unwrap().wrapping_add(1);

    let error = inspect_existing_socket(&socket_path, other_uid).unwrap_err();
    assert!(error.to_string().contains("socket uid"), "{error}");
    assert!(fs::symlink_metadata(&socket_path)
        .unwrap()
        .file_type()
        .is_socket());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn listener_refuses_live_socket_regular_file_and_symlinked_parent() {
    let data_dir = temp_dir("rf");
    fs::create_dir_all(data_dir.join("run")).unwrap();
    let socket_path = data_dir.join("run/sentineld.sock");

    let live = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
    let error = UnixSocketServer::bind(&socket_path, SecurityAudit::new(&data_dir)).unwrap_err();
    assert!(error.to_string().contains("another daemon"), "{error}");
    drop(live);
    fs::remove_file(&socket_path).unwrap();

    fs::write(&socket_path, b"not a socket").unwrap();
    let error = UnixSocketServer::bind(&socket_path, SecurityAudit::new(&data_dir)).unwrap_err();
    assert!(error.to_string().contains("not a Unix socket"), "{error}");
    assert_eq!(fs::read(&socket_path).unwrap(), b"not a socket");

    let real = data_dir.join("elsewhere");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, data_dir.join("linked")).unwrap();
    let error = UnixSocketServer::bind(
        &data_dir.join("linked/sentineld.sock"),
        SecurityAudit::new(&data_dir),
    )
    .unwrap_err();
    assert!(error.to_string().contains("symlink"), "{error}");
    assert!(!real.join("sentineld.sock").exists());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn shutdown_does_not_remove_a_socket_that_replaced_ours() {
    let data_dir = temp_dir("rp");
    fs::create_dir_all(&data_dir).unwrap();
    let socket_path = data_dir.join("run/sentineld.sock");
    let server = UnixSocketServer::bind(&socket_path, SecurityAudit::new(&data_dir)).unwrap();

    fs::remove_file(&socket_path).unwrap();
    let replacement = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
    drop(server);

    assert!(fs::symlink_metadata(&socket_path)
        .unwrap()
        .file_type()
        .is_socket());
    drop(replacement);
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
#[ignore = "uses real Unix socket peer credentials"]
async fn local_socket_integration() {
    let data_dir = temp_dir("it");
    fs::create_dir_all(&data_dir).unwrap();
    let socket_path = data_dir.join("run/sentineld.sock");
    let server = UnixSocketServer::bind(&socket_path, SecurityAudit::new(&data_dir)).unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(server.serve(shutdown_rx));

    let mut bad = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    bad.write_all(&4_u32.to_be_bytes()).await.unwrap();
    bad.write_all(b"{bad").await.unwrap();
    let mut length = [0_u8; 4];
    bad.read_exact(&mut length).await.unwrap();
    let mut response = vec![0; u32::from_be_bytes(length) as usize];
    bad.read_exact(&mut response).await.unwrap();
    assert!(String::from_utf8(response)
        .unwrap()
        .contains("INVALID_REQUEST"));

    // The listener keeps serving after a malformed frame.
    let mut client = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    let request = Envelope::new(LocalRequest::AdoptProcess {
        pid: std::process::id(),
        title: None,
        log_path: None,
    })
    .encode()
    .unwrap();
    client
        .write_all(&u32::try_from(request.len()).unwrap().to_be_bytes())
        .await
        .unwrap();
    client.write_all(request.as_bytes()).await.unwrap();
    let mut length = [0_u8; 4];
    client.read_exact(&mut length).await.unwrap();
    let mut response = vec![0; u32::from_be_bytes(length) as usize];
    client.read_exact(&mut response).await.unwrap();
    let response =
        Envelope::<LocalResponse>::decode(std::str::from_utf8(&response).unwrap()).unwrap();
    assert!(matches!(
        response.payload,
        LocalResponse::Error {
            code: LocalErrorCode::Unavailable,
            ..
        }
    ));
    assert_eq!(
        fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    shutdown_tx.send(true).unwrap();
    task.await.unwrap().unwrap();
    assert!(!socket_path.exists());
    fs::remove_dir_all(data_dir).unwrap();
}
