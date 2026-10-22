//! The user's login environment, for Sessions no terminal started (dashboard,
//! Recipes, continuations). sentinald runs as a service with a minimal
//! environment, so without this a dashboard shell has no Homebrew, conda, or
//! anything else `~/.zprofile`, `~/.zshenv`, or `~/.profile` sets up. Like
//! editors do, ask the login shell once per Session: `SHELL -l -c 'env -0'`.

use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// How long a login shell may take before the daemon's environment is used.
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_OUTPUT: u64 = 1024 * 1024;

/// Variables that describe the resolving shell itself, not the user's setup.
const TRANSIENT: &[&str] = &["_", "SHLVL", "PWD", "OLDPWD", "SENTINAL_DISABLE"];

/// The environment `shell` builds as a login shell, or `None` if it fails,
/// prints nothing usable, or takes longer than `timeout`.
pub fn resolve(shell: &Path, timeout: Duration) -> Option<Vec<(String, String)>> {
    let mut child = Command::new(shell)
        .args(["-l", "-c", "exec /usr/bin/env -0"])
        // Never auto-wrap or prompt inside the resolving shell.
        .env("SENTINAL_DISABLE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_OUTPUT)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    parse(&reader.join().ok()?.ok()?)
}

fn parse(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    let env: Vec<(String, String)> = bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| std::str::from_utf8(entry).ok())
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, _)| !name.is_empty() && !name.contains('\n'))
        .filter(|(name, _)| !TRANSIENT.contains(name))
        .filter(|(name, _)| !protocol::session_env::daemon_owned(name))
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    // A login environment always has PATH; anything else is not one.
    env.iter().any(|(name, _)| name == "PATH").then_some(env)
}

#[cfg(test)]
mod tests {
    use super::{parse, resolve};
    use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

    fn script(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sentinal-login-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn a_login_shell_provides_its_profile_environment() {
        // Stands in for a shell whose profile adds Homebrew: `-l -c CMD`.
        let shell = script(
            "profile-shell",
            "export PATH=\"/opt/homebrew/bin:$PATH\" FROM_PROFILE='two\nlines' SENTINAL_SESSION_ID=9\nshift\nexec /bin/sh \"$@\"",
        );
        let env = resolve(&shell, Duration::from_secs(5)).unwrap();
        let get = |name: &str| {
            env.iter()
                .find(|(key, _)| key == name)
                .map(|(_, v)| v.as_str())
        };
        assert!(get("PATH").unwrap().starts_with("/opt/homebrew/bin:"));
        assert_eq!(get("FROM_PROFILE"), Some("two\nlines"));
        assert_eq!(get("SENTINAL_SESSION_ID"), None, "daemon-owned");
        assert_eq!(
            get("SENTINAL_DISABLE"),
            None,
            "only for the resolving shell"
        );
    }

    #[test]
    fn a_failing_or_hanging_shell_gives_nothing() {
        assert_eq!(
            resolve(&script("failing-shell", "exit 1"), Duration::from_secs(5)),
            None
        );
        let started = std::time::Instant::now();
        assert_eq!(
            resolve(
                &script("slow-shell", "exec sleep 30"),
                Duration::from_millis(200)
            ),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            resolve(
                std::path::Path::new("/nonexistent/shell"),
                Duration::from_secs(1)
            ),
            None
        );
    }

    #[test]
    fn output_without_path_is_not_an_environment() {
        assert_eq!(parse(b"motd text\0"), None);
        assert_eq!(parse(b"HOME=/home/me\0PATH=/usr/bin\0").unwrap().len(), 2);
    }
}
