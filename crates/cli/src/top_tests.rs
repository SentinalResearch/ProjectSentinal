use super::*;
use protocol::{
    CaptureLevel, DurationMs, EventId, Gpu, Health, HealthLevel, HealthOverall, LossTrend,
    MachineId, MachineStatus, Os, ProcessSummary, SessionProcesses, TrainingSummary,
};

const NOW: i64 = 1_790_000_000_000;

fn snapshot(sessions: Vec<Session>) -> TopSnapshot {
    TopSnapshot {
        machine: Machine {
            id: MachineId("pop".into()),
            name: "pop-os".into(),
            os: Os::Linux,
            cpu: "AMD Ryzen 5 9600X 6-Core Processor".into(),
            cpu_cores: 12,
            ram_bytes: 32 * 1024 * 1024 * 1024,
            gpus: vec![Gpu {
                name: "NVIDIA GeForce RTX 5060 Ti".into(),
                vram_bytes: Some(16 * 1024 * 1024 * 1024),
            }],
            status: MachineStatus::Online,
            last_seen: Timestamp(NOW),
            role: None,
        },
        details: MachineDetails {
            hostname: "pop-os".into(),
            os_version: Some("Linux (Pop!_OS 24.04)".into()),
            kernel_version: None,
            daemon_version: "0.1.12".into(),
            started_at: Timestamp(NOW - 3 * 3_600_000 - 12 * 60_000),
            bind: "100.74.239.89:7420".into(),
            link: None,
            processes: vec![SessionProcesses {
                session_id: SessionId(12),
                processes: vec![
                    ProcessSummary {
                        pid: 4242,
                        parent_pid: None,
                        name: "python".into(),
                        cpu_percent: 97.5,
                        rss_bytes: 2 * 1024 * 1024 * 1024,
                        gpu_mem_bytes: None,
                    },
                    ProcessSummary {
                        pid: 4243,
                        parent_pid: Some(4242),
                        name: "dataloader".into(),
                        cpu_percent: 2.5,
                        rss_bytes: 12 * 1024 * 1024,
                        gpu_mem_bytes: None,
                    },
                ],
            }],
        },
        latest: Some(MachineMetricPoint {
            at: Timestamp(NOW),
            cpu_percent: 50.0,
            ram_used_bytes: 8 * 1024 * 1024 * 1024,
            ram_total_bytes: 32 * 1024 * 1024 * 1024,
            gpu_percent: Some(95.0),
            vram_used_bytes: Some(12 * 1024 * 1024 * 1024),
            vram_total_bytes: Some(16 * 1024 * 1024 * 1024),
        }),
        sessions,
        now: Timestamp(NOW),
    }
}

fn training_session() -> Session {
    let mut session = Session::new(
        SessionId(12),
        MachineId("pop".into()),
        "/bin/bash",
        "~/Projects/TIM",
        Timestamp(NOW - 5_580_000),
    );
    session.title = Some("Qwen Training".into());
    session.state = SessionState::Running;
    session.capture = CaptureLevel::Full;
    session.health = Some(Health {
        progress: Some(70.0),
        activity: HealthLevel::Good,
        resources: HealthLevel::Warning,
        stability: HealthLevel::Good,
        errors: 0,
        overall: HealthOverall::Healthy,
        evidence: vec![
            "progress reached 10.0%".into(),
            "resources below 90%".into(),
            "progress reached 13.3%".into(),
            "resources below 90%".into(),
            "loss improving for 5 checks".into(),
        ],
    });
    session.training = Some(TrainingSummary {
        loss: Some(0.362),
        first_loss: Some(1.84),
        best_val_loss: Some(0.41),
        learning_rate: Some(0.0002),
        trend: LossTrend::Improving,
    });
    session
}

#[test]
fn top_prints_machine_bars_and_live_sessions_only() {
    let mut finished = Session::new(
        SessionId(3),
        MachineId("pop".into()),
        "/bin/bash",
        "/",
        Timestamp(NOW),
    );
    finished.state = SessionState::Completed;
    let text = render_top(&snapshot(vec![training_session(), finished]));

    assert!(
        text.starts_with("pop-os · Linux (Pop!_OS 24.04) · Sentinal 0.1.12 · up 3h 12m\n"),
        "{text}"
    );
    assert!(text.contains(
        "AMD Ryzen 5 9600X 6-Core Processor, 32.0 GB, NVIDIA GeForce RTX 5060 Ti 16.0 GB"
    ));
    assert!(
        text.contains("CPU       [██████████░░░░░░░░░░] 50.0 %"),
        "{text}"
    );
    assert!(text.contains("RAM       [█████░░░░░░░░░░░░░░░] 8.0 GB / 32.0 GB"));
    assert!(text.contains("GPU       [███████████████████░] 95.0 %"));
    assert!(text.contains("VRAM      [███████████████░░░░░] 12.0 GB / 16.0 GB"));
    assert!(text.contains("SESSIONS (1 live of 2)"));
    let row = text
        .lines()
        .find(|line| line.starts_with("12 "))
        .unwrap_or_default();
    for part in [
        "RUNNING",
        "FULL",
        "Qwen Training",
        "70 %",
        "0.362 ↓ (from 1.84)",
        "100.0%",
        "2.0 GB",
        "HEALTHY",
    ] {
        assert!(row.contains(part), "{part} missing from {row:?}");
    }
    assert!(!text.lines().any(|line| line.starts_with("3 ")));
    let header = text
        .lines()
        .find(|line| line.starts_with("ID"))
        .unwrap_or_default();
    // Columns line up by character, not byte ("↓" is three bytes).
    let column_end = |line: &str, label: &str| {
        line.find(label)
            .map(|at| line[..at].chars().count() + label.chars().count())
    };
    assert_eq!(
        column_end(header, "CPU"),
        column_end(row, "100.0%"),
        "CPU column misaligned:\n{header}\n{row}"
    );
}

#[test]
fn top_says_when_nothing_runs_or_no_samples_exist() {
    let mut empty = snapshot(Vec::new());
    empty.latest = None;
    let text = render_top(&empty);
    assert!(text.contains("No Machine samples yet."));
    assert!(text.contains("Nothing is running."));
}

#[test]
fn status_shows_progress_training_health_processes_and_recent_events() {
    let events = vec![
        Event {
            id: EventId(1),
            session_id: SessionId(12),
            command_id: None,
            at: Timestamp(NOW - 3_600_000),
            kind: EventKind::CheckpointCreated {
                path: "ckpt/epoch_60.pt".into(),
            },
        },
        Event {
            id: EventId(2),
            session_id: SessionId(12),
            command_id: None,
            at: Timestamp(NOW - 60_000),
            kind: EventKind::ProgressUpdated {
                current: 70,
                total: Some(100),
                unit: "epoch".into(),
            },
        },
        Event {
            id: EventId(3),
            session_id: SessionId(12),
            command_id: None,
            at: Timestamp(NOW),
            kind: EventKind::PossiblyStalled {
                silent_for_ms: DurationMs(120_000),
            },
        },
    ];
    let snapshot = snapshot(vec![training_session()]);
    let text = render_status(&snapshot, &snapshot.sessions[0], &events);

    assert!(
        text.starts_with("Session 12 · Qwen Training · RUNNING · FULL\n"),
        "{text}"
    );
    assert!(
        text.contains("PROGRESS  [██████████████░░░░░░] epoch 70/100"),
        "{text}"
    );
    assert!(
        text.contains("LOSS 0.362 ↓ (from 1.84) · best val 0.41 · lr 2e-4"),
        "{text}"
    );
    assert!(text.contains(
        "HEALTH HEALTHY · activity GOOD · resources WARNING · stability GOOD · 0 error(s)"
    ));
    assert!(text.contains("  - loss improving for 5 checks"));
    assert_eq!(text.matches("resources below 90%").count(), 1, "{text}");
    assert!(
        !text.contains("progress reached 10.0%"),
        "older progress evidence is superseded: {text}"
    );
    assert!(text.contains("12 MB"), "{text}");
    assert!(text.contains("python"));
    assert!(text.contains("CHECKPOINT_CREATED ckpt/epoch_60.pt"));
    assert!(text.contains("POSSIBLY_STALLED   silent 2m"));
}

#[test]
fn long_output_lines_in_events_are_clipped() {
    let message = "x".repeat(200);
    let line = describe(&EventKind::ErrorDetected { message });
    assert!(line.chars().count() < 90, "{line}");
    assert!(line.ends_with('…'));
}
