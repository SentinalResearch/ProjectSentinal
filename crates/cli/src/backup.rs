//! `sentinal backup create|restore` (4.E §9). The backup boundary is the
//! config and data directories: credentials, Machine ID, password hash, VAPID
//! key, SQLite, Session logs, and audits. `create` stops the service for a
//! consistent copy and always starts it again; `restore` writes only into a
//! stopped install with no data, and only the two expected directories.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

/// Where an installation keeps its state.
#[derive(Debug, Clone)]
pub struct BackupPaths {
    /// Home directory; archive entries are relative to it.
    pub home: PathBuf,
    /// `~/.config/terminal-agent` (or as configured).
    pub config_dir: PathBuf,
    /// `~/.local/share/terminal-agent` (or `data_dir`).
    pub data_dir: PathBuf,
}

/// The user service, replaceable in tests.
pub trait ServiceControl {
    /// Stop `sentinald` and wait until it has stopped.
    fn stop(&mut self) -> Result<(), String>;
    /// Start `sentinald`.
    fn start(&mut self) -> Result<(), String>;
    /// Whether `sentinald` is running.
    fn is_running(&mut self) -> bool;
}

impl BackupPaths {
    fn relative(&self) -> Result<[PathBuf; 2], String> {
        let relative = |path: &Path| {
            path.strip_prefix(&self.home)
                .map(Path::to_path_buf)
                .map_err(|_| {
                    format!(
                        "{} is outside the home directory; back it up by hand",
                        path.display()
                    )
                })
        };
        Ok([relative(&self.config_dir)?, relative(&self.data_dir)?])
    }
}

/// Write an owner-only `tar.gz` of both directories to `output`.
pub fn create(
    paths: &BackupPaths,
    output: &Path,
    service: &mut dyn ServiceControl,
    out: &mut dyn Write,
) -> Result<(), String> {
    let members = paths.relative()?;
    let output = absolute(output)?;
    if output.starts_with(&paths.config_dir) || output.starts_with(&paths.data_dir) {
        return Err("write the backup outside the config and data directories".into());
    }
    if output.exists() {
        return Err(format!(
            "{} already exists; choose a new file",
            output.display()
        ));
    }
    let was_running = service.is_running();
    if was_running {
        let _ = writeln!(
            out,
            "Stopping sentinald for a consistent copy (live FULL Sessions are interrupted)…"
        );
        service.stop()?;
    }
    let result = archive(paths, &members, &output);
    if was_running {
        if let Err(error) = service.start() {
            let _ = writeln!(
                out,
                "sentinald did not start again ({error}); run sentinal doctor"
            );
        }
    }
    result?;
    let _ = writeln!(
        out,
        "Backup written to {}.\nIt holds this Machine's credentials, password hash, and terminal output: keep it private and off shared storage.",
        output.display()
    );
    Ok(())
}

fn archive(paths: &BackupPaths, members: &[PathBuf; 2], output: &Path) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    // Create the file 0600 first so tar never writes a readable archive.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output)
        .map_err(|error| format!("cannot create {}: {error}", output.display()))?;
    let status = Command::new("tar")
        .arg("-czf")
        .arg(output)
        .arg("-C")
        .arg(&paths.home)
        .args(members)
        .status()
        .map_err(|error| format!("cannot run tar: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(output);
        Err(format!("tar failed ({status}); no backup was written"))
    }
}

/// Restore an archive into a stopped install with no data.
pub fn restore(
    paths: &BackupPaths,
    archive: &Path,
    service: &mut dyn ServiceControl,
    out: &mut dyn Write,
) -> Result<(), String> {
    if service.is_running() {
        return Err(
            "stop sentinald first (systemctl --user stop sentinald, or launchctl bootout on macOS)"
                .into(),
        );
    }
    for dir in [&paths.config_dir, &paths.data_dir] {
        let has_state = std::fs::read_dir(dir).is_ok_and(|mut entries| {
            entries.any(|entry| entry.is_ok_and(|entry| entry.file_name() != "daemon.toml"))
        });
        if has_state {
            return Err(format!(
                "{} is not empty; restore only onto a fresh install (move it aside first)",
                dir.display()
            ));
        }
    }
    let members = paths.relative()?;
    check_entries(archive, &members)?;
    let status = Command::new("tar")
        .arg("-xzpf")
        .arg(archive)
        .arg("-C")
        .arg(&paths.home)
        .status()
        .map_err(|error| format!("cannot run tar: {error}"))?;
    if !status.success() {
        return Err(format!("tar failed ({status}) while restoring"));
    }
    for dir in [&paths.config_dir, &paths.data_dir] {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let _ = writeln!(
        out,
        "Restored {} and {}. Start sentinald; workers reconnect with the same Machine ID and credentials.",
        paths.config_dir.display(),
        paths.data_dir.display()
    );
    Ok(())
}

/// Every entry must be a regular file or directory under one of `members`.
fn check_entries(archive: &Path, members: &[PathBuf; 2]) -> Result<(), String> {
    let listing = Command::new("tar")
        .arg("-tvzf")
        .arg(archive)
        .output()
        .map_err(|error| format!("cannot run tar: {error}"))?;
    if !listing.status.success() {
        return Err(format!("{} is not a readable backup", archive.display()));
    }
    let names = Command::new("tar")
        .arg("-tzf")
        .arg(archive)
        .output()
        .map_err(|error| format!("cannot run tar: {error}"))?;
    let verbose = String::from_utf8_lossy(&listing.stdout);
    for line in verbose.lines() {
        if !matches!(line.chars().next(), Some('-' | 'd')) {
            return Err(format!(
                "the backup contains a link or special file: {line}"
            ));
        }
    }
    for name in String::from_utf8_lossy(&names.stdout).lines() {
        let path = Path::new(name.trim_end_matches('/'));
        let safe = path.is_relative()
            && !path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
            && members
                .iter()
                .any(|member| path.starts_with(member) || member.starts_with(path));
        if !safe {
            return Err(format!(
                "the backup contains '{name}', which is not Sentinal state; nothing was restored"
            ));
        }
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| format!("cannot resolve {}: {error}", path.display()))
    }
}

/// The real user service.
pub struct UserService;

impl ServiceControl for UserService {
    fn stop(&mut self) -> Result<(), String> {
        if cfg!(target_os = "macos") {
            run("launchctl", &["bootout", &launchd_target()])?;
            for _ in 0..600 {
                if !self.is_running() {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err("sentinald did not stop within 30 s".into())
        } else {
            run("systemctl", &["--user", "stop", "sentinald.service"])
        }
    }

    fn start(&mut self) -> Result<(), String> {
        if cfg!(target_os = "macos") {
            let home = std::env::var("HOME").unwrap_or_default();
            let plist = format!("{home}/Library/LaunchAgents/dev.sentinal.sentinald.plist");
            run(
                "launchctl",
                &["bootstrap", &format!("gui/{}", uid()), &plist],
            )
        } else {
            run("systemctl", &["--user", "start", "sentinald.service"])
        }
    }

    fn is_running(&mut self) -> bool {
        if cfg!(target_os = "macos") {
            run("launchctl", &["print", &launchd_target()]).is_ok()
        } else {
            run(
                "systemctl",
                &["--user", "is-active", "--quiet", "sentinald.service"],
            )
            .is_ok()
        }
    }
}

fn uid() -> String {
    Command::new("id")
        .arg("-u")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

fn launchd_target() -> String {
    format!("gui/{}/dev.sentinal.sentinald", uid())
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
