//! T0.3: after a panic the daemon starts the watcher over with the same settings.

use protocol::CaptureLevel;
use watcher::EventEngine;

#[test]
fn a_restarted_engine_keeps_its_settings_and_counts_restarts() {
    let engine = EventEngine::new(None, Vec::new())
        .with_capture(CaptureLevel::ProcessLog)
        .with_command_activity_required();
    assert_eq!(engine.restarts(), 0);

    let restarted = engine.restarted();
    assert_eq!(restarted.restarts(), 1);
    assert_eq!(restarted.capture(), CaptureLevel::ProcessLog);
    assert!(restarted.command_activity_required());
    assert_eq!(restarted.restarted().restarts(), 2);
}
