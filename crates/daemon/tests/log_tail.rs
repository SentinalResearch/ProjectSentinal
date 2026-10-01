//! Adopted log-tail behavior tests.

#![allow(clippy::unwrap_used)]

use daemon::log_tail::{LogCursor, LogTail, LogTailError, LogTailNotice, LogTailPolicy};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_file(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "sentinal-log-tail-{name}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    directory.join("process.log")
}

fn cleanup(path: &Path) {
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

fn append(path: &Path, value: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(value).unwrap();
}

fn uid(path: &Path) -> u32 {
    fs::metadata(path).unwrap().uid()
}

fn policy() -> LogTailPolicy {
    LogTailPolicy {
        read_bytes: 1024,
        initial_tail_bytes: 1024,
        max_backlog_bytes: 4096,
    }
}

fn open(path: &Path) -> LogTail {
    LogTail::open(path.to_path_buf(), uid(path), policy(), None).unwrap()
}

#[test]
fn tail_reads_append_and_preserves_partial_bytes() {
    let path = temp_file("append");
    append(&path, b"step 1");
    let mut tail = open(&path);
    let first = tail.poll().unwrap();
    assert_eq!(first.bytes, b"step 1");
    assert!(!first.reset);

    append(&path, b"/10\n");
    let second = tail.poll().unwrap();
    assert_eq!(second.bytes, b"/10\n");
    assert!(!second.reset);
    cleanup(&path);
}

#[test]
fn carriage_return_progress_bytes_pass_through_unchanged() {
    let path = temp_file("cr");
    append(&path, b"epoch 1 10%\repoch 1 20%\r");
    let mut tail = open(&path);
    assert_eq!(tail.poll().unwrap().bytes, b"epoch 1 10%\repoch 1 20%\r");
    cleanup(&path);
}

#[test]
fn invalid_utf8_bytes_are_returned_raw_and_the_file_is_untouched() {
    let path = temp_file("utf8");
    append(&path, b"loss \xff\xfe 0.4\n");
    let before = fs::metadata(&path).unwrap();
    let mut tail = open(&path);

    assert_eq!(tail.poll().unwrap().bytes, b"loss \xff\xfe 0.4\n");

    let after = fs::metadata(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"loss \xff\xfe 0.4\n");
    assert_eq!(after.permissions().mode(), before.permissions().mode());
    assert_eq!(after.len(), before.len());
    cleanup(&path);
}

#[test]
fn large_existing_log_starts_at_a_bounded_line_aligned_tail() {
    let path = temp_file("initial");
    let history: Vec<u8> = (0..400)
        .flat_map(|n| format!("line {n:04}\n").into_bytes())
        .collect();
    append(&path, &history);
    let mut tail = open(&path);

    let chunk = tail.poll().unwrap();

    assert!(chunk.bytes.len() <= 1024);
    assert!(chunk.bytes.starts_with(b"line "), "partial first line kept");
    assert!(chunk.bytes.ends_with(b"line 0399\n"));
    assert!(matches!(
        chunk.notices.as_slice(),
        [LogTailNotice::InitialTail { skipped_bytes }] if *skipped_bytes > 0
    ));
    cleanup(&path);
}

#[test]
fn small_existing_log_is_read_from_the_beginning() {
    let path = temp_file("small");
    append(&path, b"started\n");
    let mut tail = open(&path);
    let chunk = tail.poll().unwrap();
    assert_eq!(chunk.bytes, b"started\n");
    assert!(chunk.notices.is_empty());
    cleanup(&path);
}

#[test]
fn truncation_restarts_at_zero_and_resets_partial_state() {
    let path = temp_file("truncate");
    append(&path, b"old output\n");
    let mut tail = open(&path);
    assert_eq!(tail.poll().unwrap().bytes, b"old output\n");

    fs::write(&path, b"new\n").unwrap();
    let truncated = tail.poll().unwrap();

    assert!(truncated.reset);
    assert_eq!(truncated.bytes, b"new\n");
    assert_eq!(truncated.notices, vec![LogTailNotice::Truncated]);
    cleanup(&path);
}

#[test]
fn copy_truncate_rotation_keeps_reading_the_same_file_from_zero() {
    let path = temp_file("copytruncate");
    append(&path, b"before rotation\n");
    let mut tail = open(&path);
    tail.poll().unwrap();

    fs::copy(&path, path.with_extension("log.1")).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(0)
        .unwrap();
    append(&path, b"after\n");
    let chunk = tail.poll().unwrap();

    assert!(chunk.reset);
    assert_eq!(chunk.bytes, b"after\n");
    cleanup(&path);
}

#[test]
fn rename_rotation_drains_the_old_file_before_switching() {
    let path = temp_file("rename");
    append(&path, b"one\n");
    let mut tail = open(&path);
    assert_eq!(tail.poll().unwrap().bytes, b"one\n");

    let rotated = path.with_extension("log.1");
    fs::rename(&path, &rotated).unwrap();
    append(&rotated, b"late write to old file\n");
    fs::write(&path, b"fresh\n").unwrap();

    let old = tail.poll().unwrap();
    assert_eq!(old.bytes, b"late write to old file\n");
    assert!(!old.reset);
    let switched = tail.poll().unwrap();
    assert!(switched.reset);
    assert_eq!(switched.bytes, b"fresh\n");
    assert_eq!(switched.notices, vec![LogTailNotice::Replaced]);
    cleanup(&path);
}

#[test]
fn same_size_replacement_is_detected_by_file_identity() {
    let path = temp_file("same-size");
    append(&path, b"aaaa\n");
    let mut tail = open(&path);
    tail.poll().unwrap();

    let replacement = path.with_extension("tmp");
    fs::write(&replacement, b"bbbb\n").unwrap();
    fs::rename(&replacement, &path).unwrap();
    let chunk = tail.poll().unwrap();

    assert!(chunk.reset);
    assert_eq!(chunk.bytes, b"bbbb\n");
    cleanup(&path);
}

#[test]
fn missing_file_is_temporary_and_a_recreated_file_is_read() {
    let path = temp_file("missing");
    append(&path, b"first\n");
    let mut tail = open(&path);
    tail.poll().unwrap();

    fs::remove_file(&path).unwrap();
    let missing = tail.poll().unwrap();
    assert!(missing.bytes.is_empty());
    assert_eq!(missing.notices, vec![LogTailNotice::Missing]);
    assert!(
        tail.poll().unwrap().notices.is_empty(),
        "missing is reported once"
    );

    fs::write(&path, b"second\n").unwrap();
    let recreated = tail.poll().unwrap();
    assert!(recreated.reset);
    assert_eq!(recreated.bytes, b"second\n");
    cleanup(&path);
}

#[test]
fn each_poll_is_bounded_without_losing_remaining_bytes() {
    let path = temp_file("bounded");
    fs::write(&path, b"123456789").unwrap();
    let mut tail = LogTail::open(
        path.clone(),
        uid(&path),
        LogTailPolicy {
            read_bytes: 4,
            ..policy()
        },
        None,
    )
    .unwrap();
    assert_eq!(tail.poll().unwrap().bytes, b"1234");
    assert_eq!(tail.poll().unwrap().bytes, b"5678");
    assert_eq!(tail.poll().unwrap().bytes, b"9");
    cleanup(&path);
}

#[test]
fn long_line_is_still_read_in_bounded_chunks() {
    let path = temp_file("long-line");
    append(&path, b"");
    let mut tail = open(&path);
    append(&path, &vec![b'x'; 3000]);
    let total: usize = (0..3).map(|_| tail.poll().unwrap().bytes.len()).sum();
    assert_eq!(total, 3000);
    cleanup(&path);
}

#[test]
fn backlog_over_the_limit_is_skipped_with_a_notice() {
    let path = temp_file("backlog");
    append(&path, b"start\n");
    let mut tail = open(&path);
    tail.poll().unwrap();

    let burst: Vec<u8> = (0..1000)
        .flat_map(|n| format!("burst {n:04}\n").into_bytes())
        .collect();
    append(&path, &burst);
    let chunk = tail.poll().unwrap();

    assert!(chunk.reset);
    assert!(matches!(
        chunk.notices.as_slice(),
        [LogTailNotice::BacklogSkipped { skipped_bytes }] if *skipped_bytes > 4096
    ));
    assert!(chunk.bytes.starts_with(b"burst "));
    let rest: usize = (0..4).map(|_| tail.poll().unwrap().bytes.len()).sum();
    assert!(chunk.bytes.len() + rest <= 1024 + 16);
    cleanup(&path);
}

#[test]
fn drain_reads_every_remaining_byte_up_to_the_backlog_bound() {
    let path = temp_file("drain");
    append(&path, b"a\n");
    let mut tail = LogTail::open(
        path.clone(),
        uid(&path),
        LogTailPolicy {
            read_bytes: 2,
            ..policy()
        },
        None,
    )
    .unwrap();
    tail.poll().unwrap();
    append(&path, b"bb\ncc\nfinal");

    let drained = tail.drain().unwrap();

    assert_eq!(drained.bytes, b"bb\ncc\nfinal");
    cleanup(&path);
}

#[test]
fn cursor_resumes_without_replaying_read_bytes() {
    let path = temp_file("cursor");
    append(&path, b"seen\n");
    let mut tail = open(&path);
    tail.poll().unwrap();
    let cursor = tail.cursor();
    append(&path, b"new\n");

    let mut resumed = LogTail::open(path.clone(), uid(&path), policy(), Some(cursor)).unwrap();
    let chunk = resumed.poll().unwrap();

    assert_eq!(chunk.bytes, b"new\n");
    assert_eq!(chunk.notices, vec![LogTailNotice::Resumed]);
    cleanup(&path);
}

#[test]
fn cursor_for_another_file_identity_falls_back_to_the_initial_tail() {
    let path = temp_file("cursor-mismatch");
    append(&path, b"current\n");
    let cursor = LogCursor {
        device: 0,
        inode: 0,
        offset: 3,
    };

    let mut tail = LogTail::open(path.clone(), uid(&path), policy(), Some(cursor)).unwrap();
    let chunk = tail.poll().unwrap();

    assert_eq!(chunk.bytes, b"current\n");
    assert_eq!(chunk.notices, vec![LogTailNotice::CursorMismatch]);
    cleanup(&path);
}

#[test]
fn cursor_past_the_end_of_a_truncated_file_falls_back() {
    let path = temp_file("cursor-short");
    append(&path, b"0123456789\n");
    let mut tail = open(&path);
    tail.poll().unwrap();
    let cursor = tail.cursor();
    fs::write(&path, b"x\n").unwrap();

    let mut resumed = LogTail::open(path.clone(), uid(&path), policy(), Some(cursor)).unwrap();
    let chunk = resumed.poll().unwrap();

    assert_eq!(chunk.bytes, b"x\n");
    assert_eq!(chunk.notices, vec![LogTailNotice::CursorMismatch]);
    cleanup(&path);
}

#[test]
fn opening_a_symlink_is_refused() {
    let path = temp_file("symlink-open");
    let target = path.with_extension("target");
    fs::write(&target, b"secret\n").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();

    let error = LogTail::open(path.clone(), uid(&target), policy(), None)
        .err()
        .unwrap();

    assert!(matches!(error, LogTailError::Unsafe { .. }), "{error}");
    cleanup(&path);
}

#[test]
fn opening_a_file_owned_by_another_uid_is_refused() {
    let path = temp_file("owner");
    append(&path, b"x\n");

    let error = LogTail::open(path.clone(), uid(&path) + 1, policy(), None)
        .err()
        .unwrap();

    assert!(matches!(error, LogTailError::Unsafe { .. }), "{error}");
    cleanup(&path);
}

#[test]
fn path_replaced_by_a_symlink_stops_the_tail() {
    let path = temp_file("symlink-swap");
    append(&path, b"x\n");
    let mut tail = open(&path);
    tail.poll().unwrap();
    let target = path.with_extension("target");
    fs::write(&target, b"secret\n").unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();

    let error = tail.poll().err().unwrap();

    assert!(matches!(error, LogTailError::Unsafe { .. }), "{error}");
    cleanup(&path);
}

#[test]
fn path_replaced_by_a_directory_stops_the_tail() {
    let path = temp_file("type-swap");
    append(&path, b"x\n");
    let mut tail = open(&path);
    tail.poll().unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();

    let error = tail.poll().err().unwrap();

    assert!(matches!(error, LogTailError::Unsafe { .. }), "{error}");
    cleanup(&path);
}

#[test]
fn unreadable_replacement_stops_the_tail_with_a_file_error() {
    let path = temp_file("permission");
    append(&path, b"x\n");
    let mut tail = open(&path);
    tail.poll().unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(&path, b"hidden\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

    let error = tail.poll().err().unwrap();

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(error, LogTailError::File { .. }), "{error}");
    cleanup(&path);
}

#[test]
fn already_open_file_keeps_reading_after_its_mode_is_removed() {
    let path = temp_file("chmod-open");
    append(&path, b"a\n");
    let mut tail = open(&path);
    tail.poll().unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o200)).unwrap();
    append(&path, b"b\n");

    let chunk = tail.poll();

    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(chunk.unwrap().bytes, b"b\n");
    cleanup(&path);
}

#[test]
fn zero_read_limit_is_rejected() {
    let path = temp_file("zero");
    append(&path, b"x\n");
    let error = LogTail::open(
        path.clone(),
        uid(&path),
        LogTailPolicy {
            read_bytes: 0,
            ..policy()
        },
        None,
    )
    .err()
    .unwrap();
    assert!(matches!(error, LogTailError::ZeroLimit { .. }));
    cleanup(&path);
}
