//! Generic watcher behavior tests.

use protocol::{Command, CommandId, DurationMs, EventKind, MachineId, SessionId, Timestamp};
use watcher::{EventEngine, GenericAdapter, Observation, ObservationKind, WatcherAdapter};

#[test]
fn prompt_requires_exact_shape_and_two_quiet_seconds() {
    let mut generic = GenericAdapter::new(DurationMs(600_000));
    assert!(generic
        .observe(&Observation::output(Timestamp(0), "Continue? [y/N]"))
        .is_empty());
    assert!(generic
        .observe(&Observation::tick(Timestamp(1_999)))
        .is_empty());
    assert!(matches!(
        generic.observe(&Observation::tick(Timestamp(2_000))).as_slice(),
        [EventKind::WaitingForInput { prompt: Some(prompt), .. }] if prompt == "Continue? [y/N]"
    ));

    let mut negative = GenericAdapter::new(DurationMs(600_000));
    negative.observe(&Observation::output(
        Timestamp(0),
        "the password is stored in a keychain",
    ));
    assert!(negative
        .observe(&Observation::tick(Timestamp(3_000)))
        .is_empty());
}

#[test]
fn prompts_with_the_usual_trailing_space_still_wait() {
    for (line, expected) in [
        ("Continue? [y/N] ", "Continue? [y/N]"),
        ("Proceed (yes/no) ", "Proceed (yes/no)"),
        ("Password:  ", "Password:"),
        ("Overwrite file?\t ", "Overwrite file?"),
    ] {
        let mut generic = GenericAdapter::new(DurationMs(600_000));
        generic.observe(&Observation::output(Timestamp(0), line));
        assert!(
            matches!(
                generic.observe(&Observation::tick(Timestamp(2_000))).as_slice(),
                [EventKind::WaitingForInput { prompt: Some(prompt), .. }] if prompt == expected
            ),
            "{line:?} should be a waiting prompt"
        );
    }

    let mut finished = GenericAdapter::new(DurationMs(600_000));
    finished.observe(&Observation::output(Timestamp(0), "Did it work? yes "));
    assert!(finished
        .observe(&Observation::tick(Timestamp(3_000)))
        .is_empty());
}

#[test]
fn idle_cpu_and_gpu_stall_but_busy_gpu_does_not() {
    let mut stalled = GenericAdapter::new(DurationMs(600_000));
    stalled.observe(&Observation::output(Timestamp(0), "working"));
    let events = stalled.observe(&Observation::metrics(Timestamp(600_000), 2.0, Some(1.0)));
    assert!(matches!(
        events.as_slice(),
        [EventKind::PossiblyStalled { .. }]
    ));

    let mut computing = GenericAdapter::new(DurationMs(600_000));
    computing.observe(&Observation::output(Timestamp(0), "working"));
    assert!(computing
        .observe(&Observation::metrics(Timestamp(600_000), 1.0, Some(96.0)))
        .is_empty());
}

#[test]
fn metrics_start_the_quiet_clock_when_no_output_has_ever_arrived() {
    let mut generic = GenericAdapter::new(DurationMs(600_000));
    assert!(generic
        .observe(&Observation::metrics(Timestamp(0), 1.0, None))
        .is_empty());
    assert!(matches!(
        generic
            .observe(&Observation::metrics(Timestamp(600_000), 1.0, None))
            .as_slice(),
        [EventKind::PossiblyStalled { .. }]
    ));
}

#[test]
fn generic_detects_errors_and_warnings() {
    let mut generic = GenericAdapter::new(DurationMs(600_000));
    let error = generic.observe(&Observation::output(
        Timestamp(0),
        "RuntimeError: CUDA out of memory",
    ));
    assert!(matches!(
        error.as_slice(),
        [EventKind::ErrorDetected { .. }]
    ));
    let warning = generic.observe(&Observation::output(Timestamp(1), "WARNING: retrying"));
    assert!(matches!(
        warning.as_slice(),
        [EventKind::WarningDetected { .. }]
    ));
}

#[test]
fn command_gated_engine_never_calls_an_idle_shell_stalled() {
    let mut engine = EventEngine::new(None, Vec::new()).with_command_activity_required();

    engine.observe(&Observation::output(Timestamp(0), "shell prompt"));
    let before_command = engine.observe(&Observation::metrics(Timestamp(600_000), 1.0, Some(1.0)));
    assert!(before_command.events.is_empty());

    let mut command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("machine".into()),
        "python work.py",
        "/work",
        Timestamp(700_000),
    );
    engine.observe(&Observation {
        at: Timestamp(700_000),
        kind: ObservationKind::CommandStarted(command.clone()),
    });
    engine.observe(&Observation::output(Timestamp(700_001), "working"));
    command.finish(0, Timestamp(701_000)).unwrap();
    engine.observe(&Observation {
        at: Timestamp(701_000),
        kind: ObservationKind::CommandFinished(command),
    });

    let after_command = engine.observe(&Observation::metrics(Timestamp(1_301_000), 1.0, Some(1.0)));
    assert!(after_command.events.is_empty());
}

#[test]
fn a_server_start_line_from_any_program_is_a_server_started_event() {
    for (line, expected_port, expected_url) in [
        (
            "Uvicorn running on http://0.0.0.0:8000 (Press CTRL+C to quit)",
            Some(8000),
            Some("http://0.0.0.0:8000"),
        ),
        ("INFO:     Listening on :8080", Some(8080), None),
        ("Server started on port 3000", Some(3000), None),
        (
            "listening at http://127.0.0.1:5000",
            Some(5000),
            Some("http://127.0.0.1:5000"),
        ),
        // The port is the number after the address's last colon, never a
        // part of the address or of a timestamp before the phrase.
        ("Listening on 127.0.0.1:8080", Some(8080), None),
        ("server is listening on 10.0.0.5:3000", Some(3000), None),
        (
            "2026-11-08T12:00:01Z INFO listening on [::]:9000",
            Some(9000),
            None,
        ),
        ("listening on 0.0.0.0:8081, pid 4123", Some(8081), None),
    ] {
        // A fresh engine per line: one server is reported once (see below).
        let mut engine = EventEngine::new(None, Vec::new());
        let update = engine.observe(&Observation::output(Timestamp(0), line));
        assert!(
            update.events.contains(&EventKind::ServerStarted {
                port: expected_port,
                url: expected_url.map(str::to_string),
            }),
            "missing SERVER_STARTED for: {line}"
        );
    }
}

#[test]
fn ordinary_output_is_not_a_server_start() {
    let mut engine = EventEngine::new(None, Vec::new());
    for line in [
        "listening to music",
        "the server is not responding",
        "port 8080 is already in use",
        "loading model weights",
    ] {
        let update = engine.observe(&Observation::output(Timestamp(0), line));
        assert!(
            !update
                .events
                .iter()
                .any(|e| matches!(e, EventKind::ServerStarted { .. })),
            "false SERVER_STARTED for: {line}"
        );
    }
}

fn servers(update: &watcher::EngineUpdate) -> Vec<Option<u16>> {
    update
        .events
        .iter()
        .filter_map(|event| match event {
            EventKind::ServerStarted { port, .. } => Some(*port),
            _ => None,
        })
        .collect()
}

#[test]
fn a_server_is_reported_once_per_port_per_command() {
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("machine".into()),
        "npm run dev",
        "/work",
        Timestamp(0),
    );
    let mut engine = EventEngine::new(Some(&command), Vec::new());
    let first = engine.observe(&Observation::output(
        Timestamp(1),
        "Listening on 127.0.0.1:8080",
    ));
    assert_eq!(servers(&first), vec![Some(8080)]);
    // A dev server repeats itself on every reload.
    let again = engine.observe(&Observation::output(
        Timestamp(2),
        "INFO: listening on :8080",
    ));
    assert!(servers(&again).is_empty());
    let other = engine.observe(&Observation::output(
        Timestamp(3),
        "listening on 0.0.0.0:9090",
    ));
    assert_eq!(servers(&other), vec![Some(9090)]);

    let mut next = command.clone();
    next.id = CommandId(2);
    engine.observe(&Observation {
        at: Timestamp(4),
        kind: ObservationKind::CommandStarted(next),
    });
    let restarted = engine.observe(&Observation::output(
        Timestamp(5),
        "Listening on 127.0.0.1:8080",
    ));
    assert_eq!(
        servers(&restarted),
        vec![Some(8080)],
        "a new Command reports it again"
    );
}
