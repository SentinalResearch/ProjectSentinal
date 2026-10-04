use super::*;
use axum::{body::Bytes, extract::State, http::HeaderMap, routing::post, Router};
use protocol::{CaptureLevel, DurationMs, EventId, Resource};
use std::sync::atomic::{AtomicI64, Ordering};

type CapturedRequest = Arc<Mutex<Option<tokio::sync::oneshot::Sender<(HeaderMap, Bytes)>>>>;

fn event(kind: EventKind) -> Event {
    Event {
        id: EventId(1),
        session_id: SessionId(7),
        command_id: None,
        at: Timestamp(0),
        kind,
    }
}

#[tokio::test]
async fn defaults_overrides_and_fake_clock_dedup_are_deterministic() {
    let now = Arc::new(AtomicI64::new(1_000));
    let clock_now = now.clone();
    let root = std::env::temp_dir().join(format!("sentinal-notify-{}", std::process::id()));
    let (service, diagnostics) = NotificationService::load(
        &root.join("missing.toml"),
        root.join("rules.json"),
        MachineId("machine-a".into()),
        Arc::new(move || Timestamp(clock_now.load(Ordering::SeqCst))),
    );
    assert!(diagnostics.is_empty());
    let stalled = event(EventKind::PossiblyStalled {
        silent_for_ms: DurationMs(60_000),
    });
    assert!(
        !service
            .process(&stalled, CaptureLevel::Full, "A", "train")
            .await
    );
    assert!(
        !service
            .process(&stalled, CaptureLevel::Full, "A", "train")
            .await
    );
    assert_eq!(service.journal().len(), 1);
    now.store(301_000, Ordering::SeqCst);
    assert!(
        !service
            .process(&stalled, CaptureLevel::Full, "A", "train")
            .await
    );
    assert_eq!(service.journal().len(), 2);

    service
        .set_rule(
            Some(SessionId(7)),
            NotificationRuleSet {
                stalled: false,
                progress: true,
                ..NotificationRuleSet::default()
            },
        )
        .unwrap();
    let progress = event(EventKind::ProgressUpdated {
        current: 2,
        total: Some(10),
        unit: "epoch".into(),
    });
    assert!(
        !service
            .process(&progress, CaptureLevel::Full, "A", "train")
            .await
    );
    assert_eq!(service.journal().len(), 3);
    assert!(
        !service
            .process(&stalled, CaptureLevel::Full, "A", "train")
            .await
    );
    assert_eq!(service.journal().len(), 3);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn ntfy_client_posts_only_structured_message_to_local_fake_server() {
    let (sent, received) = tokio::sync::oneshot::channel();
    let sent = Arc::new(Mutex::new(Some(sent)));
    async fn receive(State(sent): State<CapturedRequest>, headers: HeaderMap, body: Bytes) {
        if let Ok(mut sender) = sent.lock() {
            if let Some(sender) = sender.take() {
                let _ = sender.send((headers, body));
            }
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/sentinal-test", post(receive))
                .with_state(sent),
        )
        .await
        .unwrap();
    });
    let root = std::env::temp_dir().join(format!("sentinal-notify-http-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("notify.toml"),
        format!("server='http://{address}'\ntopic='sentinal-test'\ntoken='private-token'\n"),
    )
    .unwrap();
    let (service, diagnostics) = NotificationService::load(
        &root.join("notify.toml"),
        root.join("rules.json"),
        MachineId("machine-a".into()),
        Arc::new(|| Timestamp(1_000)),
    );
    assert!(diagnostics.is_empty());
    let finished = event(EventKind::CommandFinished {
        exit_code: 0,
        duration_ms: DurationMs(20),
    });
    assert!(
        service
            .process(&finished, CaptureLevel::Full, "RTX PC", "cargo test")
            .await
    );
    let (headers, body) = received.await.unwrap();
    assert_eq!(
        headers.get("authorization").unwrap(),
        "Bearer private-token"
    );
    assert_eq!(body, "RTX PC · cargo test: command finished with exit 0");
    assert!(!String::from_utf8_lossy(&body).contains("raw secret output"));
    server.abort();
    let _ = fs::remove_dir_all(root);
}

fn quiet_service(name: &str) -> (NotificationService, PathBuf) {
    let root = std::env::temp_dir().join(format!("sentinal-notify-{name}-{}", std::process::id()));
    let (service, _) = NotificationService::load(
        &root.join("missing.toml"),
        root.join("rules.json"),
        MachineId("machine-a".into()),
        Arc::new(|| Timestamp(1_000)),
    );
    (service, root)
}

#[test]
fn default_rule_set_matches_notifies_by_default() {
    let kinds = [
        EventKind::CommandFinished {
            exit_code: 0,
            duration_ms: DurationMs(1),
        },
        EventKind::CommandFinished {
            exit_code: 2,
            duration_ms: DurationMs(1),
        },
        EventKind::ErrorDetected {
            message: "boom".into(),
        },
        EventKind::WarningDetected {
            message: "careful".into(),
        },
        EventKind::WaitingForInput { prompt: None },
        EventKind::PossiblyStalled {
            silent_for_ms: DurationMs(1),
        },
        EventKind::ResourceHigh {
            resource: Resource::Gpu,
            percent: 96.0,
        },
        EventKind::ResourceHigh {
            resource: Resource::Gpu,
            percent: 91.0,
        },
        EventKind::CheckpointCreated {
            path: "ckpt".into(),
        },
        EventKind::ProgressUpdated {
            current: 1,
            total: None,
            unit: "step".into(),
        },
    ];
    for kind in kinds {
        assert_eq!(
            category_enabled(NotificationRuleSet::default(), &kind),
            kind.notifies_by_default(),
            "{kind:?}"
        );
    }
}

#[tokio::test]
async fn error_text_never_includes_the_raw_output_line() {
    let (service, root) = quiet_service("raw");
    let error = event(EventKind::ErrorDetected {
        message: "AWS_SECRET_ACCESS_KEY=hunter2 failed".into(),
    });
    service
        .process(&error, CaptureLevel::Full, "A", "train")
        .await;
    let journal = service.journal();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].text, "A · train: error detected");
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn capture_without_evidence_for_the_kind_never_notifies() {
    let (service, root) = quiet_service("capture");
    let error = event(EventKind::ErrorDetected {
        message: "x".into(),
    });
    let finished = event(EventKind::CommandFinished {
        exit_code: 1,
        duration_ms: DurationMs(1),
    });
    service
        .process(&error, CaptureLevel::Commands, "A", "train")
        .await;
    service
        .process(&finished, CaptureLevel::ProcessLog, "A", "train")
        .await;
    assert!(service.journal().is_empty());
    service
        .process(&finished, CaptureLevel::Commands, "A", "train")
        .await;
    assert_eq!(service.journal().len(), 1);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn stored_default_rule_applies_to_sessions_without_override() {
    let (service, root) = quiet_service("default");
    service
        .set_rule(
            None,
            NotificationRuleSet {
                finished: false,
                ..NotificationRuleSet::default()
            },
        )
        .unwrap();
    let finished = event(EventKind::CommandFinished {
        exit_code: 0,
        duration_ms: DurationMs(1),
    });
    service
        .process(&finished, CaptureLevel::Full, "A", "train")
        .await;
    assert!(service.journal().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn plain_http_server_is_accepted_only_for_loopback_or_tailnet() {
    let config = |server: &str| NotifyConfig {
        server: server.into(),
        topic: "t".into(),
        token: None,
    };
    for ok in [
        "https://ntfy.sh",
        "http://127.0.0.1:8080",
        "http://localhost:80",
        "http://[::1]:2586",
        "http://100.74.239.89:2586",
    ] {
        assert!(valid_config(&config(ok)), "{ok}");
    }
    for bad in [
        "http://ntfy.example.com",
        "http://192.168.1.4",
        "http://100.200.1.1",
        "ftp://x",
        "https://user:pw@ntfy.sh",
    ] {
        assert!(!valid_config(&config(bad)), "{bad}");
    }
}
