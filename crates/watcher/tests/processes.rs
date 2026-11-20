//! Process observation Event tests.

use protocol::{Process, ProcessState, Timestamp};
use watcher::{EventEngine, Observation, ObservationKind, ProcessObservation};

fn process(pid: u32, name: &str) -> Process {
    Process {
        pid,
        parent_pid: None,
        name: name.into(),
        cmdline: Vec::new(),
        state: ProcessState::Running,
        cpu_percent: 0.0,
        rss_bytes: 0,
        gpu_mem_bytes: None,
        started_at: Timestamp(1),
    }
}

#[test]
fn process_samples_emit_started_and_exited_differences_once() {
    let mut engine = EventEngine::new(None, Vec::new());
    let first = engine.observe(&Observation {
        at: Timestamp(1),
        kind: ObservationKind::ProcessSample(ProcessObservation {
            processes: vec![process(10, "shell"), process(11, "python")],
        }),
    });
    assert_eq!(first.events.len(), 2);
    let unchanged = engine.observe(&Observation {
        at: Timestamp(2),
        kind: ObservationKind::ProcessSample(ProcessObservation {
            processes: vec![process(10, "shell"), process(11, "python")],
        }),
    });
    assert!(unchanged.events.is_empty());
    let exited = engine.observe(&Observation {
        at: Timestamp(3),
        kind: ObservationKind::ProcessSample(ProcessObservation {
            processes: vec![process(10, "shell")],
        }),
    });
    assert!(matches!(
        exited.events.as_slice(),
        [protocol::EventKind::ProcessExited {
            pid: 11,
            exit_code: None
        }]
    ));
}

#[test]
fn a_terminal_that_tracks_commands_has_no_event_per_child_process() {
    // QA 2026-11-19: every `sleep` in a script added two Events.
    let mut engine = EventEngine::new(None, Vec::new()).with_command_activity_required();
    for (at, processes) in [
        (1, vec![process(10, "zsh")]),
        (2, vec![process(10, "zsh"), process(11, "sleep")]),
        (3, vec![process(10, "zsh")]),
    ] {
        let update = engine.observe(&Observation {
            at: Timestamp(at),
            kind: ObservationKind::ProcessSample(ProcessObservation { processes }),
        });
        assert!(update.events.is_empty(), "at {at}: {:?}", update.events);
    }
}

#[test]
fn short_lived_workers_of_an_adopted_run_are_not_events() {
    // Field report 2026-11-19: one PROCESS_STARTED per EvalPlus worker.
    let mut engine = EventEngine::new(None, Vec::new());
    let sample = |at: i64, processes: Vec<Process>| Observation {
        at: Timestamp(at),
        kind: ObservationKind::ProcessSample(ProcessObservation { processes }),
    };
    // The adopted run itself is reported at once.
    assert_eq!(
        engine
            .observe(&sample(0, vec![process(10, "python")]))
            .events
            .len(),
        1
    );
    // A worker that lives 2 s: nothing, coming or going.
    assert!(engine
        .observe(&sample(
            1_000,
            vec![process(10, "python"), process(11, "worker")]
        ))
        .events
        .is_empty());
    assert!(engine
        .observe(&sample(3_000, vec![process(10, "python")]))
        .events
        .is_empty());
    // A worker that stays is reported once it has lived 5 s.
    engine.observe(&sample(
        4_000,
        vec![process(10, "python"), process(12, "eval")],
    ));
    let later = engine.observe(&sample(
        9_000,
        vec![process(10, "python"), process(12, "eval")],
    ));
    assert_eq!(later.events.len(), 1);
}
