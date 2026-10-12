use super::*;
use protocol::{
    DurationMs, EventId, NotificationCategory, NotificationDeliveryState, NotificationResultClass,
    Resource, Session, SessionState,
};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Mutex,
    },
};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn event(id: u64, kind: EventKind) -> Event {
    Event {
        id: EventId(id),
        session_id: SessionId(7),
        command_id: None,
        at: Timestamp(1_000),
        kind,
    }
}

fn private_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sentinal-notify-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    root
}

fn write_config(root: &Path, server: &str, token: Option<&str>) -> PathBuf {
    let path = root.join("notify.toml");
    let token = token.map_or_else(String::new, |value| format!("token='{value}'\n"));
    fs::write(
        &path,
        format!("server='{server}'\ntopic='sentinal-test'\n{token}"),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    path
}

fn store_with_session() -> Arc<Store> {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut session = Session::new(
        SessionId(7),
        MachineId("machine-a".into()),
        "/bin/sh",
        "/work",
        Timestamp(1),
    );
    session.title = Some("Training".into());
    session
        .transition(SessionState::Idle, Timestamp(2))
        .unwrap();
    store.insert_session(&session).unwrap();
    store
}

fn service(
    root: &Path,
    store: Arc<Store>,
    clock: NotificationClock,
) -> (NotificationService, Vec<String>) {
    NotificationService::load(
        &root.join("notify.toml"),
        root.join("rules.json"),
        MachineId("machine-a".into()),
        "RTX PC".into(),
        fs::metadata(root).unwrap().uid(),
        true,
        store,
        Arc::new(|| Some("https://control.tailnet.ts.net".into())),
        clock,
    )
}

#[test]
fn configuration_enforces_mode_values_and_redaction() {
    let root = private_root("config");
    let path = write_config(&root, "https://ntfy.example", Some("private-token"));
    let store = store_with_session();
    let (service, diagnostics) = service(&root, store, Arc::new(|| Timestamp(1_000)));
    assert!(diagnostics.is_empty());
    let status = service.status();
    assert!(status.configured);
    assert_eq!(
        status.configuration_state,
        Some(NotificationConfigurationState::Configured)
    );
    assert!(status.token_configured);
    assert!(!format!("{status:?}").contains("private-token"));

    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let status = service.reload_config();
    assert_eq!(
        status.configuration_state,
        Some(NotificationConfigurationState::Invalid)
    );
    assert!(status.diagnostic.unwrap().contains("0600"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_config_is_unavailable_and_symlink_is_invalid() {
    let root = private_root("missing");
    let store = store_with_session();
    let (service, diagnostics) = service(&root, store, Arc::new(|| Timestamp(1_000)));
    assert!(diagnostics.is_empty());
    assert_eq!(
        service.status().configuration_state,
        Some(NotificationConfigurationState::Unavailable)
    );
    let target = root.join("actual.toml");
    fs::write(&target, "server='https://ntfy.example'\ntopic='t'\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&target, root.join("notify.toml")).unwrap();
    let status = service.reload_config();
    assert_eq!(
        status.configuration_state,
        Some(NotificationConfigurationState::Invalid)
    );
    assert!(status.diagnostic.unwrap().contains("symlink"));
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn exact_safe_request_uses_structured_event_and_click() {
    let root = private_root("request");
    write_config(&root, "http://127.0.0.1:2586", Some("private-token"));
    // CommandFinished only maps to Completed/Failed for a `sentinal watch`
    // Session (one defining Command) — see notifications.rs::enqueue. An
    // ordinary (default-origin) Session runs many Commands, so this exact
    // request-shape test needs a Watch-origin Session to legitimately notify;
    // built inline (not store_with_session(), shared by tests that don't care
    // about origin) so only this test's precondition changes.
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut session = Session::new(
        SessionId(7),
        MachineId("machine-a".into()),
        "/bin/sh",
        "/work",
        Timestamp(1),
    );
    session.title = Some("Training".into());
    session.origin = SessionOrigin::Watch;
    session
        .transition(SessionState::Idle, Timestamp(2))
        .unwrap();
    store.insert_session(&session).unwrap();
    let finished = event(
        1,
        EventKind::CommandFinished {
            exit_code: 0,
            duration_ms: DurationMs(20),
        },
    );
    store.insert_event(&finished).unwrap();
    let (service, diagnostics) = service(&root, store, Arc::new(|| Timestamp(1_000)));
    assert!(diagnostics.is_empty());
    let captured = Arc::new(Mutex::new(None));
    let sender_capture = captured.clone();
    let sender: NotificationSender = Arc::new(move |request| {
        if let Ok(mut value) = sender_capture.lock() {
            *value = Some(request);
        }
        Box::pin(async { SendResult::Accepted })
    });
    let service = service.with_sender(sender);
    assert!(service.enqueue(&finished, CaptureLevel::Full).unwrap());
    assert_eq!(service.deliver_due_once().await.unwrap(), 1);
    let request = captured.lock().unwrap().clone().unwrap();
    assert_eq!(request.url, "http://127.0.0.1:2586/sentinal-test");
    assert_eq!(request.token.as_deref(), Some("private-token"));
    assert_eq!(request.presentation.title, "Sentinal · RTX PC");
    assert_eq!(request.presentation.body, "Training — completed (exit 0)");
    assert_eq!(request.presentation.tag, "white_check_mark");
    assert_eq!(request.presentation.priority, "default");
    assert_eq!(
        request.presentation.click.as_deref(),
        Some("https://control.tailnet.ts.net/app/sessions/machine-a%3A7")
    );
    let journal = service.journal();
    assert_eq!(journal[0].state, Some(NotificationDeliveryState::Delivered));
    assert_eq!(
        journal[0].result_class,
        Some(NotificationResultClass::Accepted)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn presentation_never_copies_output_prompt_command_or_checkpoint_path() {
    let cases = [
        EventKind::ErrorDetected {
            message: "AWS_SECRET_ACCESS_KEY=hunter2".into(),
        },
        EventKind::WaitingForInput {
            prompt: Some("Password: top-secret".into()),
        },
        EventKind::CheckpointCreated {
            path: "/private/models/secret.safetensors".into(),
        },
    ];
    for kind in cases {
        let rendered = presentation::render(
            &kind,
            "RTX PC",
            None,
            &MachineId("machine-a".into()),
            SessionId(7),
            None,
        )
        .unwrap();
        let joined = format!("{} {}", rendered.title, rendered.body);
        assert!(!joined.contains("hunter2"));
        assert!(!joined.contains("Password"));
        assert!(!joined.contains("/private"));
        assert!(rendered.click.is_none());
    }
}

#[test]
fn native_sink_receives_only_closed_structured_facts() {
    let root = private_root("native");
    let store = store_with_session();
    let detected = event(
        9,
        EventKind::ErrorDetected {
            message: "secret terminal output".into(),
        },
    );
    store.insert_event(&detected).unwrap();
    let (service, _) = service(&root, store, Arc::new(|| Timestamp(2_000)));
    let captured = Arc::new(Mutex::new(None));
    let capture = captured.clone();
    let service = service.with_native_sink(Arc::new(move |intent, _| {
        *capture.lock().unwrap() = Some(intent.clone());
        Ok(Some(NativeNotificationState::ForwardQueued))
    }));

    assert!(service.enqueue(&detected, CaptureLevel::Full).unwrap());
    let intent = captured.lock().unwrap().clone().unwrap();
    assert_eq!(intent.facts, NotificationFacts::Error);
    let encoded = serde_json::to_string(&intent).unwrap();
    assert!(!encoded.contains("secret terminal output"));
    assert_eq!(
        service.journal()[0].state,
        Some(NotificationDeliveryState::ForwardQueued)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_supported_category_has_stable_appearance() {
    let cases = [
        (
            EventKind::CommandFinished {
                exit_code: 0,
                duration_ms: DurationMs(1),
            },
            NotificationCategory::Completed,
            "white_check_mark",
        ),
        (
            EventKind::CommandFinished {
                exit_code: 3,
                duration_ms: DurationMs(1),
            },
            NotificationCategory::Failed,
            "x",
        ),
        (
            EventKind::ErrorDetected {
                message: "x".into(),
            },
            NotificationCategory::Error,
            "rotating_light",
        ),
        (
            EventKind::WaitingForInput { prompt: None },
            NotificationCategory::Waiting,
            "hourglass_flowing_sand",
        ),
        (
            EventKind::PossiblyStalled {
                silent_for_ms: DurationMs(600_000),
            },
            NotificationCategory::Stalled,
            "warning",
        ),
        (
            EventKind::ResourceHigh {
                resource: Resource::Gpu,
                percent: 90.0,
            },
            NotificationCategory::ResourceHigh,
            "fire",
        ),
        (
            EventKind::CheckpointCreated { path: "x".into() },
            NotificationCategory::Checkpoint,
            "floppy_disk",
        ),
        (
            EventKind::ProgressUpdated {
                current: 37,
                total: Some(100),
                unit: "epoch".into(),
            },
            NotificationCategory::Progress,
            "chart_with_upwards_trend",
        ),
    ];
    for (kind, category, tag) in cases {
        let rendered = presentation::render(
            &kind,
            "M",
            Some("S"),
            &MachineId("m".into()),
            SessionId(1),
            None,
        )
        .unwrap();
        assert_eq!(rendered.category, category);
        assert_eq!(rendered.tag, tag);
    }
}

#[tokio::test]
async fn transient_failure_retries_without_blocking_dedup_state() {
    let root = private_root("retry");
    write_config(&root, "http://127.0.0.1:2586", None);
    let store = store_with_session();
    let stalled = event(
        1,
        EventKind::PossiblyStalled {
            silent_for_ms: DurationMs(600_000),
        },
    );
    store.insert_event(&stalled).unwrap();
    let now = Arc::new(AtomicI64::new(1_000));
    let clock_now = now.clone();
    let (service, _) = service(
        &root,
        store,
        Arc::new(move || Timestamp(clock_now.load(Ordering::SeqCst))),
    );
    let outcomes = Arc::new(Mutex::new(vec![SendResult::Accepted, SendResult::Timeout]));
    let sender_outcomes = outcomes.clone();
    let sender: NotificationSender = Arc::new(move |_| {
        let result = sender_outcomes.lock().unwrap().pop().unwrap();
        Box::pin(async move { result })
    });
    let service = service.with_sender(sender);
    assert!(service.enqueue(&stalled, CaptureLevel::Full).unwrap());
    assert_eq!(service.deliver_due_once().await.unwrap(), 0);
    let retry = &service.journal()[0];
    assert_eq!(retry.state, Some(NotificationDeliveryState::Retrying));
    assert_eq!(retry.attempts, Some(1));
    now.store(6_000, Ordering::SeqCst);
    assert_eq!(service.deliver_due_once().await.unwrap(), 1);
    assert_eq!(
        service.journal()[0].state,
        Some(NotificationDeliveryState::Delivered)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn capture_rules_and_missing_configuration_are_visible() {
    let root = private_root("rules");
    let store = store_with_session();
    let error = event(
        1,
        EventKind::ErrorDetected {
            message: "x".into(),
        },
    );
    store.insert_event(&error).unwrap();
    let (service, _) = service(&root, store, Arc::new(|| Timestamp(1_000)));
    assert!(!service.enqueue(&error, CaptureLevel::Commands).unwrap());
    assert!(service.journal().is_empty());
    assert!(!service.enqueue(&error, CaptureLevel::Full).unwrap());
    assert_eq!(
        service.journal()[0].state,
        Some(NotificationDeliveryState::NotConfigured)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn config_parent_and_file_are_owned_by_the_test_user() {
    let root = private_root("owner");
    let path = write_config(&root, "https://ntfy.example", None);
    assert_eq!(
        fs::metadata(&root).unwrap().uid(),
        fs::metadata(&path).unwrap().uid()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn only_watch_jobs_and_long_commands_notify_when_they_finish() {
    use super::{finish_notifies, LONG_COMMAND_MS};
    assert!(finish_notifies(Some(SessionOrigin::Watch), 5));
    assert!(finish_notifies(None, 5));
    for origin in [
        SessionOrigin::Ui,
        SessionOrigin::ShellHook,
        SessionOrigin::AutoWrap,
    ] {
        assert!(
            !finish_notifies(Some(origin), 2_000),
            "{origin:?}: `ls` must not notify"
        );
        assert!(!finish_notifies(Some(origin), LONG_COMMAND_MS - 1));
        assert!(
            finish_notifies(Some(origin), LONG_COMMAND_MS),
            "{origin:?}: a long build notifies"
        );
    }
}
