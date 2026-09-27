//! Behaviour tests: whole scenarios from the spec, across several types.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{
    Command, CommandId, CommandStatus, DurationMs, Event, EventId, EventKind, Session, SessionId,
    SessionState, Timestamp,
};

const MIN: i64 = 60_000;

/// The Milestone 1 story: start a job, walk away (disconnect), come back.
#[test]
fn long_job_survives_a_disconnect() {
    let mut session = Session::new(SessionId(1), "rtx-pc".into(), "/bin/zsh", "~", Timestamp(0));
    session
        .transition(SessionState::Idle, Timestamp(100))
        .unwrap();

    // User runs a long job.
    let mut job = Command::start(
        CommandId(1),
        session.id,
        session.machine_id.clone(),
        "python long_job.py",
        "~",
        Timestamp(MIN),
    );
    session.current_command = Some(job.id);
    session
        .transition(SessionState::Running, Timestamp(MIN))
        .unwrap();

    // Laptop closes: the viewer loses contact.
    session
        .transition(SessionState::Disconnected, Timestamp(2 * MIN))
        .unwrap();
    assert_eq!(
        session.current_command,
        Some(job.id),
        "job must not be forgotten"
    );

    // Phone reconnects: job is still running.
    session
        .transition(SessionState::Running, Timestamp(30 * MIN))
        .unwrap();
    assert_eq!(job.status(), CommandStatus::Running);

    // Job finishes and the shell goes back to the prompt.
    job.finish(0, Timestamp(91 * MIN)).unwrap();
    session
        .transition(SessionState::Idle, Timestamp(91 * MIN))
        .unwrap();

    assert_eq!(job.status(), CommandStatus::Succeeded);
    assert_eq!(job.duration(Timestamp(999 * MIN)).to_string(), "1h30m");
    assert_eq!(session.current_command, None);
}

/// A failing test run produces the events the UI and notifier rely on.
#[test]
fn failed_test_run_produces_a_notifying_error_event() {
    let mut cmd = Command::start(
        CommandId(2),
        SessionId(1),
        "rtx-pc".into(),
        "pytest tests/",
        "~/proj",
        Timestamp(0),
    );
    cmd.finish(1, Timestamp(42_000)).unwrap();

    let event = Event {
        id: EventId(1),
        session_id: cmd.session_id,
        command_id: Some(cmd.id),
        at: cmd.finished_at.unwrap(),
        kind: EventKind::CommandFinished {
            exit_code: cmd.exit_code.unwrap(),
            duration_ms: cmd.duration(Timestamp(0)),
        },
    };

    assert_eq!(cmd.duration(Timestamp(0)), DurationMs(42_000));
    assert_eq!(event.kind.severity(), protocol::Severity::Error);
    assert!(event.kind.notifies_by_default());
}

/// Killing a session is final.
#[test]
fn killed_session_stays_killed() {
    let mut session = Session::new(
        SessionId(3),
        "mac-mini".into(),
        "/bin/bash",
        "~",
        Timestamp(0),
    );
    session
        .transition(SessionState::Idle, Timestamp(1))
        .unwrap();
    session
        .transition(SessionState::Running, Timestamp(2))
        .unwrap();
    session
        .transition(SessionState::Interrupted, Timestamp(3))
        .unwrap();

    for next in SessionState::ALL {
        if next != SessionState::Interrupted {
            assert!(
                session.transition(next, Timestamp(4)).is_err(),
                "-> {next:?}"
            );
        }
    }
}
