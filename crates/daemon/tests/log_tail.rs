//! Adopted log-tail behavior tests.

#![allow(clippy::unwrap_used)]

use daemon::log_tail::LogTail;
use std::{
    fs,
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_file(name: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "sentinal-log-tail-{name}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    directory.join("process.log")
}

fn append(path: &std::path::Path, value: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(value).unwrap();
}

#[test]
fn tail_reads_append_and_preserves_partial_bytes() {
    let path = temp_file("append");
    append(&path, b"step 1");
    let mut tail = LogTail::open(path.clone(), 1024).unwrap();
    let first = tail.poll().unwrap();
    assert_eq!(first.bytes, b"step 1");
    assert!(!first.reset);

    append(&path, b"/10\n");
    let second = tail.poll().unwrap();
    assert_eq!(second.bytes, b"/10\n");
    assert!(!second.reset);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn tail_resets_on_truncate_and_rotation() {
    let path = temp_file("reset");
    append(&path, b"old output\n");
    let mut tail = LogTail::open(path.clone(), 1024).unwrap();
    assert_eq!(tail.poll().unwrap().bytes, b"old output\n");

    fs::write(&path, b"new\n").unwrap();
    let truncated = tail.poll().unwrap();
    assert!(truncated.reset);
    assert_eq!(truncated.bytes, b"new\n");

    let rotated = path.with_extension("log.1");
    fs::rename(&path, &rotated).unwrap();
    fs::write(&path, b"rotated\n").unwrap();
    let replacement = tail.poll().unwrap();
    assert!(replacement.reset);
    assert_eq!(replacement.bytes, b"rotated\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn tail_bounds_each_read_without_losing_remaining_bytes() {
    let path = temp_file("bounded");
    fs::write(&path, b"123456789").unwrap();
    let mut tail = LogTail::open(path.clone(), 4).unwrap();
    assert_eq!(tail.poll().unwrap().bytes, b"1234");
    assert_eq!(tail.poll().unwrap().bytes, b"5678");
    assert_eq!(tail.poll().unwrap().bytes, b"9");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
