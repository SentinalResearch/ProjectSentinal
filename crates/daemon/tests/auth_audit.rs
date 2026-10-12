//! Authentication and remote-input audit behaviour tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::{AuditLog, AuditRecord},
    auth::{constant_time_eq, load_or_create_token, AuthError, AuthManager, CredentialKind},
};
use protocol::{SessionId, Timestamp};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc,
    },
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "terminal-agent-{name}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn generated_token_is_persisted_and_reused() {
    let dir = temp_dir("token");
    let path = dir.join("token");

    let first = load_or_create_token(&path).unwrap();
    let second = load_or_create_token(&path).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn generated_token_is_owner_read_write_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("token-mode");
    let path = dir.join("token");
    load_or_create_token(&path).unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

    assert_eq!(mode, 0o600);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn token_comparison_accepts_only_an_exact_match() {
    assert!(constant_time_eq("abc123", "abc123"));
    assert!(!constant_time_eq("abc123", "abc124"));
    assert!(!constant_time_eq("abc123", "abc1234"));
}

fn auth_manager(dir: &std::path::Path, now: Arc<AtomicI64>) -> AuthManager {
    AuthManager::load_with_clock(
        &dir.join("auth.json"),
        "bootstrap-token".into(),
        86_400_000,
        Arc::new(move || Timestamp(now.load(Ordering::Relaxed))),
    )
    .unwrap()
}

#[tokio::test]
async fn password_setup_persists_only_an_argon2id_hash() {
    let dir = temp_dir("password-setup");
    let now = Arc::new(AtomicI64::new(1_790_000_000_000));
    let auth = auth_manager(&dir, now.clone());

    assert!(auth.setup_required().unwrap());
    let session = auth
        .setup("bootstrap-token", "correct horse battery staple")
        .await
        .unwrap();

    assert_eq!(
        auth.authorize(&session.token).unwrap(),
        Some(CredentialKind::BrowserSession)
    );
    let stored = fs::read_to_string(dir.join("auth.json")).unwrap();
    assert!(stored.contains("$argon2id$"));
    assert!(!stored.contains("correct horse battery staple"));

    let reloaded = auth_manager(&dir, now);
    assert!(!reloaded.setup_required().unwrap());
    assert!(reloaded
        .login("100.64.0.2", "correct horse battery staple")
        .await
        .is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn password_file_is_owner_read_write_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("password-mode");
    let auth = auth_manager(&dir, Arc::new(AtomicI64::new(1_790_000_000_000)));
    auth.setup("bootstrap-token", "correct horse battery staple")
        .await
        .unwrap();

    let mode = fs::metadata(dir.join("auth.json"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn setup_requires_bootstrap_token_and_a_strong_password() {
    let dir = temp_dir("setup-validation");
    let auth = auth_manager(&dir, Arc::new(AtomicI64::new(1_790_000_000_000)));

    assert!(matches!(
        auth.setup("wrong-token", "correct horse battery staple")
            .await,
        Err(AuthError::InvalidBootstrap)
    ));
    assert!(matches!(
        auth.setup("bootstrap-token", "too-short").await,
        Err(AuthError::InvalidPassword { .. })
    ));
    assert!(auth.setup_required().unwrap());
}

#[tokio::test]
async fn fifth_bad_password_locks_only_that_client_until_clock_advances() {
    let dir = temp_dir("login-lockout");
    let now = Arc::new(AtomicI64::new(1_790_000_000_000));
    let auth = auth_manager(&dir, now.clone());
    auth.setup("bootstrap-token", "correct horse battery staple")
        .await
        .unwrap();

    for remaining in (1..=4).rev() {
        assert!(matches!(
            auth.login("100.64.0.2", "wrong password").await,
            Err(AuthError::InvalidCredentials { attempts_remaining })
                if attempts_remaining == remaining
        ));
    }
    let retry_at = match auth.login("100.64.0.2", "wrong password").await {
        Err(AuthError::Locked { retry_at }) => retry_at,
        other => panic!("expected lockout, got {other:?}"),
    };
    assert!(matches!(
        auth.login("100.64.0.2", "correct horse battery staple")
            .await,
        Err(AuthError::Locked { .. })
    ));
    assert!(auth
        .login("100.64.0.3", "correct horse battery staple")
        .await
        .is_ok());

    now.store(retry_at.0 + 1, Ordering::Relaxed);
    assert!(auth
        .login("100.64.0.2", "correct horse battery staple")
        .await
        .is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn rotating_client_keys_hit_a_global_failure_budget() {
    // F10: behind Tailscale Serve the client key is the loopback
    // `Tailscale-User-Login` header, which a local process can vary freely.
    let dir = temp_dir("login-global-budget");
    let now = Arc::new(AtomicI64::new(1_790_000_000_000));
    let auth = auth_manager(&dir, now.clone());
    auth.setup("bootstrap-token", "correct horse battery staple")
        .await
        .unwrap();

    let mut locked_at = None;
    for attempt in 0..20 {
        match auth
            .login(&format!("rotating-{attempt}@example"), "wrong password")
            .await
        {
            Err(AuthError::InvalidCredentials { .. }) => {}
            Err(AuthError::Locked { retry_at }) => {
                locked_at = Some(retry_at);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    let retry_at = locked_at.expect("20 failures across rotating keys must lock every client");
    assert!(matches!(
        auth.login("a-fresh-key@example", "correct horse battery staple")
            .await,
        Err(AuthError::Locked { .. })
    ));
    now.store(retry_at.0 + 1, Ordering::Relaxed);
    assert!(auth
        .login("a-fresh-key@example", "correct horse battery staple")
        .await
        .is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn browser_session_expires_without_sleeping() {
    let dir = temp_dir("session-expiry");
    let now = Arc::new(AtomicI64::new(1_790_000_000_000));
    let auth = auth_manager(&dir, now.clone());
    let session = auth
        .setup("bootstrap-token", "correct horse battery staple")
        .await
        .unwrap();

    now.store(session.expires_at.0, Ordering::Relaxed);

    assert_eq!(auth.authorize(&session.token).unwrap(), None);
    assert!(matches!(
        auth.session_info(&session.token),
        Err(AuthError::InvalidSession)
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn password_change_revokes_old_sessions_and_returns_a_new_one() {
    let dir = temp_dir("password-change");
    let auth = auth_manager(&dir, Arc::new(AtomicI64::new(1_790_000_000_000)));
    let old_session = auth
        .setup("bootstrap-token", "correct horse battery staple")
        .await
        .unwrap();

    let new_session = auth
        .change_password(
            &old_session.token,
            "correct horse battery staple",
            "a different secure passphrase",
        )
        .await
        .unwrap();

    assert_eq!(auth.authorize(&old_session.token).unwrap(), None);
    assert_eq!(
        auth.authorize(&new_session.token).unwrap(),
        Some(CredentialKind::BrowserSession)
    );
    assert!(matches!(
        auth.login("100.64.0.2", "correct horse battery staple")
            .await,
        Err(AuthError::InvalidCredentials { .. })
    ));
    assert!(auth
        .login("100.64.0.2", "a different secure passphrase")
        .await
        .is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_password_file_is_rejected_with_its_path() {
    let dir = temp_dir("malformed-password-file");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("auth.json");
    fs::write(&path, r#"{"password_hash":"not-an-argon-hash"}"#).unwrap();

    let error = match AuthManager::load(&path, "bootstrap-token".into()) {
        Ok(_) => panic!("malformed password file unexpectedly loaded"),
        Err(error) => error,
    };

    assert!(
        matches!(error, AuthError::PasswordData { ref path, .. } if path.ends_with("auth.json"))
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn invalid_session_cannot_logout_or_change_password() {
    let dir = temp_dir("invalid-session");
    let auth = auth_manager(&dir, Arc::new(AtomicI64::new(1_790_000_000_000)));
    auth.setup("bootstrap-token", "correct horse battery staple")
        .await
        .unwrap();

    assert!(matches!(
        auth.logout("not-a-session"),
        Err(AuthError::InvalidSession)
    ));
    assert!(matches!(
        auth.change_password(
            "not-a-session",
            "correct horse battery staple",
            "a different secure passphrase",
        )
        .await,
        Err(AuthError::InvalidSession)
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn audit_log_records_metadata_but_not_terminal_bytes() {
    let dir = temp_dir("audit");
    let log = AuditLog::new(&dir);
    let record = AuditRecord {
        timestamp: Timestamp(1_790_000_000_000),
        client_addr: "100.64.0.2:50123".into(),
        session_id: SessionId(7),
        byte_count: 19,
    };

    log.append(&record).unwrap();

    let line = fs::read_to_string(dir.join("audit.jsonl")).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"session_id\":7"));
    assert!(line.contains("\"byte_count\":19"));
    assert!(!line.contains("password=hunter2"));
    fs::remove_dir_all(dir).unwrap();
}
