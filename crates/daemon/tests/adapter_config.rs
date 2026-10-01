//! Daemon-owned declarative adapter loading tests.

#![allow(clippy::unwrap_used)]

use daemon::adapter_config::load_adapters;
use protocol::{Command, CommandId, MachineId, SessionId, Timestamp};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
use watcher::WatcherAdapter;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn directory() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sentinal-adapters-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn missing_directory_is_an_empty_adapter_set() {
    let path = directory().join("missing");
    assert!(load_adapters(&path).unwrap().is_empty());
}

#[test]
fn toml_files_load_in_name_order_and_invalid_rule_names_its_file() {
    let path = directory();
    fs::write(
        path.join("b.toml"),
        "name='b'\nmatch_command='^b '\n[[rule]]\nregex='done'\nemit='WARNING_DETECTED'\n",
    )
    .unwrap();
    fs::write(
        path.join("a.toml"),
        "name='a'\nmatch_command='^a '\n[[rule]]\nregex='step (?P<current>\\d+)'\nemit='PROGRESS_UPDATED'\n",
    )
    .unwrap();
    let adapters = load_adapters(&path).unwrap();
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("machine".into()),
        "a run",
        "/work",
        Timestamp(0),
    );
    assert_eq!(adapters.len(), 2);
    assert!(adapters[0].matches(&command));

    fs::write(
        path.join("broken.toml"),
        "name='broken'\nmatch_command='^broken '\n[[rule]]\nregex='('\nemit='ERROR_DETECTED'\n",
    )
    .unwrap();
    let error = load_adapters(&path).unwrap_err();
    assert!(error.to_string().contains("broken.toml"));
    fs::remove_dir_all(path).unwrap();
}
