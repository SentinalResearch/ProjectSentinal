//! Finding the regular file an adopted process writes its stdout/stderr to.
//!
//! Discovery only reads metadata: Linux `/proc/<pid>/fd/{1,2}` links, or
//! `lsof -F` output on macOS. Parsers are pure so they are tested with
//! sample data; the OS runners are exercised only in ignored integration.
//! Any failure means "no candidate": capture stays PROCESS.

use protocol::{LogSource, SessionLog};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Upper bound for one `lsof` run.
pub const LSOF_TIMEOUT: Duration = Duration::from_secs(2);

/// A regular-file path from one `/proc/<pid>/fd/N` link target, if any.
///
/// Pipes (`pipe:[..]`), sockets, anonymous inodes, devices under `/dev/`,
/// deleted files (`... (deleted)`), and relative text are not log candidates.
pub fn proc_link_candidate(link: &str) -> Option<String> {
    if !link.starts_with('/') || link.starts_with("/dev/") || link.ends_with(" (deleted)") {
        return None;
    }
    Some(link.to_string())
}

/// Stdout and stderr regular-file paths from `lsof -a -p PID -d 1,2 -Fftn`.
pub fn lsof_candidates(output: &str) -> (Option<String>, Option<String>) {
    let (mut stdout, mut stderr) = (None, None);
    let (mut fd, mut kind): (Option<&str>, Option<&str>) = (None, None);
    for line in output.lines() {
        let (tag, value) = line.split_at(line.len().min(1));
        match tag {
            "f" => {
                fd = Some(value);
                kind = None;
            }
            "t" => kind = Some(value),
            "n" if kind == Some("REG") && value.starts_with('/') => {
                let path = value
                    .strip_suffix(" (deleted)")
                    .map_or(Some(value), |_| None);
                match (fd, path) {
                    (Some("1"), Some(path)) => stdout = Some(path.to_string()),
                    (Some("2"), Some(path)) => stderr = Some(path.to_string()),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    (stdout, stderr)
}

/// Combine stdout/stderr candidates, keeping only regular files.
pub fn choose_log(
    stdout: Option<String>,
    stderr: Option<String>,
    is_regular_file: impl Fn(&str) -> bool,
) -> Option<SessionLog> {
    let stdout = stdout.filter(|path| is_regular_file(path));
    let stderr = stderr.filter(|path| is_regular_file(path));
    let (path, source) = match (stdout, stderr) {
        (Some(out), Some(err)) if out == err => (out, LogSource::StdoutAndStderr),
        (Some(out), _) => (out, LogSource::Stdout),
        (None, Some(err)) => (err, LogSource::Stderr),
        (None, None) => return None,
    };
    Some(SessionLog { path, source })
}

/// Discover a log for `pid` on this OS; never panics, never writes.
pub fn discover(pid: u32) -> Option<SessionLog> {
    let (stdout, stderr) = if cfg!(target_os = "linux") {
        let link = |fd: u8| {
            std::fs::read_link(format!("/proc/{pid}/fd/{fd}"))
                .ok()
                .and_then(|target| proc_link_candidate(&target.to_string_lossy()))
        };
        (link(1), link(2))
    } else if cfg!(target_os = "macos") {
        lsof_candidates(&run_lsof(pid)?)
    } else {
        return None;
    };
    choose_log(stdout, stderr, |path| {
        std::fs::metadata(Path::new(path)).is_ok_and(|metadata| metadata.is_file())
    })
}

/// Run `lsof` for fds 1 and 2 of `pid`, killing it after [`LSOF_TIMEOUT`].
fn run_lsof(pid: u32) -> Option<String> {
    let mut child = Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "1,2", "-Fftn"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + LSOF_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut output).ok()?;
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regular(path: &str) -> bool {
        !path.contains("dir")
    }

    #[test]
    fn proc_links_keep_only_regular_file_paths() {
        assert_eq!(
            proc_link_candidate("/home/u/train.log"),
            Some("/home/u/train.log".into())
        );
        for link in [
            "pipe:[123]",
            "socket:[99]",
            "anon_inode:[eventfd]",
            "/dev/pts/3",
            "/dev/null",
            "/home/u/old.log (deleted)",
        ] {
            assert_eq!(proc_link_candidate(link), None, "{link}");
        }
    }

    #[test]
    fn stdout_and_stderr_to_the_same_file_are_one_log() {
        let log = choose_log(Some("/w/a.log".into()), Some("/w/a.log".into()), regular).unwrap();
        assert_eq!(log.source, LogSource::StdoutAndStderr);
        let out = choose_log(Some("/w/o.log".into()), Some("/w/e.log".into()), regular).unwrap();
        assert_eq!(
            (out.path.as_str(), out.source),
            ("/w/o.log", LogSource::Stdout)
        );
        let err = choose_log(None, Some("/w/e.log".into()), regular).unwrap();
        assert_eq!(err.source, LogSource::Stderr);
    }

    #[test]
    fn non_regular_files_and_no_candidate_mean_no_log() {
        assert_eq!(choose_log(Some("/w/dir".into()), None, regular), None);
        assert_eq!(choose_log(None, None, regular), None);
    }

    #[test]
    fn lsof_output_yields_regular_file_stdout_and_stderr() {
        let output = "p4242\nf1\ntREG\nn/Users/u/train.log\nf2\ntREG\nn/Users/u/train.log\n";
        assert_eq!(
            lsof_candidates(output),
            (
                Some("/Users/u/train.log".into()),
                Some("/Users/u/train.log".into())
            )
        );
    }

    #[test]
    fn lsof_pipes_sockets_devices_and_deleted_files_are_ignored() {
        let output = "p1\nf1\ntPIPE\nn->0x1234\nf2\ntCHR\nn/dev/ttys003\n";
        assert_eq!(lsof_candidates(output), (None, None));
        let unix = "p1\nf1\ntunix\nn->0xabc\nf2\ntREG\nn/tmp/gone.log (deleted)\n";
        assert_eq!(lsof_candidates(unix), (None, None));
    }

    #[test]
    fn lsof_permission_errors_or_empty_output_mean_no_candidate() {
        assert_eq!(lsof_candidates(""), (None, None));
        assert_eq!(lsof_candidates("p1\n"), (None, None));
        let stderr_only = "p1\nf2\ntREG\nn/var/log/app.err\n";
        assert_eq!(
            lsof_candidates(stderr_only),
            (None, Some("/var/log/app.err".into()))
        );
    }
}
