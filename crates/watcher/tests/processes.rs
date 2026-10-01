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
