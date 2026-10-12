//! The control's daily check for a newer signed release (Milestone 4.E
//! §11b.1), done by invoking `sentinal release check` on a schedule. The
//! actual GitHub fetch, channel filtering, and signature verification stay
//! in the CLI (`crates/cli/src/release/`, already shipped and tested); this
//! module only decides when to run it and never duplicates that logic.

use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;

const BASE_INTERVAL_SECS: u64 = 24 * 60 * 60;
const JITTER_RANGE_SECS: u64 = 60 * 60;
const CHECK_TIMEOUT: Duration = Duration::from_secs(120);
const POLICY_FILE: &str = "release-policy.json";

/// Where to find the policy file and the sibling `sentinal` CLI binary this
/// control invokes for release checks.
#[derive(Debug, Clone)]
pub struct ReleaseWatchSettings {
    /// Daemon data directory, holding `release-policy.json`/`release-watch.json`.
    pub data_dir: PathBuf,
    /// Path to the `sentinal` CLI binary.
    pub sentinal_bin: PathBuf,
}

#[derive(Debug, Deserialize)]
struct PolicyField {
    policy: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CheckReport {
    current: Option<String>,
    available: Option<serde_json::Value>,
    unsigned_found: Option<String>,
}

/// What the watch acts with after each tick: the dashboard's release service
/// (to start an automatic update) and the control's Push gateway (notices).
#[derive(Clone)]
pub struct ReleaseActors {
    /// The control's release service.
    pub release: super::ControlRelease,
    /// The control's native Push gateway, when configured.
    pub push: Option<crate::push::gateway::PushGateway>,
}

/// How often the watch wakes to act (window and result checks are local).
const TICK: Duration = Duration::from_secs(15 * 60);

/// Start the background release watch; stops when `shutdown` signals true.
/// Checks GitHub promptly, then roughly every 24h (± 1h jitter). Every 15 min
/// it also announces new releases and update results once and, under
/// "Automatic when idle", starts an update inside the window (`release_actions`).
pub fn spawn_release_watch(
    settings: ReleaseWatchSettings,
    actors: Option<ReleaseActors>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut next_check = tokio::time::Instant::now();
        loop {
            if tokio::time::Instant::now() >= next_check {
                run_one_check(&settings).await;
                next_check = tokio::time::Instant::now() + jittered_interval(now_nanos());
            }
            if let Some(actors) = &actors {
                act(&settings.data_dir, actors).await;
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tokio::time::sleep(TICK) => {}
            }
        }
    });
}

/// Perform this tick's release actions and remember what was announced.
async fn act(data_dir: &Path, actors: &ReleaseActors) {
    use super::release_actions::{plan, read_notified, write_notified, ReleaseAction};
    let view = actors.release.view();
    let notified = read_notified(data_dir);
    let (actions, next) = plan(&view, &notified, local_minutes().await);
    for action in actions {
        match action {
            ReleaseAction::Notice(title, body) => {
                if let Some(push) = &actors.push {
                    push.send_control_notice(&title, &body).await;
                }
            }
            ReleaseAction::AutoUpdate(version) => {
                tracing::info!(%version, "release watch: starting the automatic control update");
                if let Err(failure) = actors.release.launch(&version, &view.current, None) {
                    tracing::warn!(message = %failure.failure.message, "release watch: automatic update did not start");
                }
            }
        }
    }
    if next != notified {
        if let Err(error) = write_notified(data_dir, &next) {
            tracing::warn!(%error, "release watch: could not record announced releases");
        }
    }
}

/// Local time of day in minutes, from `date` (no time-zone dependency).
async fn local_minutes() -> Option<u32> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new("date").arg("+%H%M").output(),
    )
    .await
    .ok()?
    .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let value: u32 = text.trim().parse().ok()?;
    let (hours, minutes) = (value / 100, value % 100);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// Read the policy and, unless it is `OFF`, run `sentinal release check
/// --json` once and log the outcome. Never panics: a failed or unparsable
/// check is logged and the next scheduled check still runs.
async fn run_one_check(settings: &ReleaseWatchSettings) {
    let policy = read_policy(&settings.data_dir).await;
    if policy.eq_ignore_ascii_case("OFF") {
        tracing::debug!("release watch: policy is OFF, skipping GitHub check");
        return;
    }

    let result = tokio::time::timeout(
        CHECK_TIMEOUT,
        Command::new(&settings.sentinal_bin)
            .args(["release", "check", "--json"])
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            match serde_json::from_str::<CheckReport>(&stdout) {
                Ok(report) => {
                    tracing::info!(
                        current = report.current.as_deref().unwrap_or("?"),
                        available = report.available.is_some(),
                        unsigned_found = report.unsigned_found.as_deref().unwrap_or(""),
                        "release watch: checked"
                    );
                }
                Err(error) => {
                    tracing::warn!(%error, output = %stdout, "release watch: unparsable check output");
                }
            }
        }
        Ok(Ok(output)) => {
            tracing::warn!(
                status = %output.status,
                stderr = %String::from_utf8_lossy(&output.stderr),
                "release watch: check failed"
            );
        }
        Ok(Err(error)) => {
            tracing::warn!(
                %error,
                bin = %settings.sentinal_bin.display(),
                "release watch: could not run sentinal release check"
            );
        }
        Err(_elapsed) => {
            tracing::warn!("release watch: check timed out");
        }
    }
}

/// Read just the `"policy"` field of `release-policy.json`; a missing or
/// unparsable file defaults to `"NOTIFY"`, matching the CLI's own default,
/// so the watch still runs until the owner explicitly turns it off.
async fn read_policy(data_dir: &Path) -> String {
    let path = data_dir.join(POLICY_FILE);
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => serde_json::from_str::<PolicyField>(&text)
            .ok()
            .and_then(|field| field.policy)
            .unwrap_or_else(|| "NOTIFY".into()),
        Err(_) => "NOTIFY".into(),
    }
}

fn now_nanos() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0)
}

/// `BASE_INTERVAL_SECS` plus a jitter in `[-JITTER_RANGE_SECS, +JITTER_RANGE_SECS]`
/// derived from `seed` (sub-second wall-clock nanoseconds in production), so
/// restarts across machines don't all poll GitHub at the same moment. No
/// `rand` dependency: this only needs to spread timing, not be unpredictable.
fn jittered_interval(seed: u32) -> Duration {
    let span = 2 * JITTER_RANGE_SECS + 1;
    let offset = u64::from(seed) % span;
    let secs = BASE_INTERVAL_SECS - JITTER_RANGE_SECS + offset;
    Duration::from_secs(secs)
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn scratch_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("release-watch-test-{label}-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn write_policy(dir: &Path, policy: &str) {
        fs::write(
            dir.join("release-policy.json"),
            format!(r#"{{"policy":"{policy}","channel":"stable","window":"03:00-05:00"}}"#),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn off_policy_never_invokes_the_binary() {
        let dir = scratch_dir("off");
        write_policy(&dir, "OFF");
        let marker = dir.join("invoked");
        let bin = dir.join("fake-sentinal");
        write_executable(
            &bin,
            &format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
        );

        run_one_check(&ReleaseWatchSettings {
            data_dir: dir.clone(),
            sentinal_bin: bin,
        })
        .await;

        assert!(
            !marker.exists(),
            "OFF policy must never invoke the release check"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn notify_policy_invokes_release_check_with_json() {
        let dir = scratch_dir("notify");
        write_policy(&dir, "NOTIFY");
        let argv_log = dir.join("argv");
        let bin = dir.join("fake-sentinal");
        write_executable(
            &bin,
            &format!(
                "#!/bin/sh\necho \"$@\" > '{}'\necho '{{\"current\":\"0.1.17\",\"channel\":\"stable\",\"available\":null,\"unsigned_found\":null}}'\nexit 0\n",
                argv_log.display()
            ),
        );

        run_one_check(&ReleaseWatchSettings {
            data_dir: dir.clone(),
            sentinal_bin: bin,
        })
        .await;

        let argv = fs::read_to_string(&argv_log).unwrap();
        assert_eq!(argv.trim(), "release check --json");
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn automatic_when_idle_policy_also_checks() {
        let dir = scratch_dir("auto");
        write_policy(&dir, "AUTOMATIC_WHEN_IDLE");
        let marker = dir.join("invoked");
        let bin = dir.join("fake-sentinal");
        write_executable(
            &bin,
            &format!(
                "#!/bin/sh\ntouch '{}'\necho '{{}}'\nexit 0\n",
                marker.display()
            ),
        );

        run_one_check(&ReleaseWatchSettings {
            data_dir: dir.clone(),
            sentinal_bin: bin,
        })
        .await;

        assert!(marker.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn missing_policy_file_defaults_to_notify_and_still_checks() {
        let dir = scratch_dir("missing");
        let marker = dir.join("invoked");
        let bin = dir.join("fake-sentinal");
        write_executable(
            &bin,
            &format!(
                "#!/bin/sh\ntouch '{}'\necho '{{}}'\nexit 0\n",
                marker.display()
            ),
        );

        run_one_check(&ReleaseWatchSettings {
            data_dir: dir.clone(),
            sentinal_bin: bin,
        })
        .await;

        assert!(marker.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn nonexistent_binary_does_not_panic_and_future_checks_still_work() {
        let dir = scratch_dir("missing-bin");
        write_policy(&dir, "NOTIFY");
        let settings = ReleaseWatchSettings {
            data_dir: dir.clone(),
            sentinal_bin: dir.join("does-not-exist"),
        };

        run_one_check(&settings).await; // must not panic

        let marker = dir.join("invoked");
        let bin = dir.join("fake-sentinal");
        write_executable(
            &bin,
            &format!(
                "#!/bin/sh\ntouch '{}'\necho '{{}}'\nexit 0\n",
                marker.display()
            ),
        );
        run_one_check(&ReleaseWatchSettings {
            data_dir: dir.clone(),
            sentinal_bin: bin,
        })
        .await;
        assert!(marker.exists(), "a later, valid check must still run");
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn nonzero_exit_does_not_panic() {
        let dir = scratch_dir("nonzero");
        write_policy(&dir, "NOTIFY");
        let bin = dir.join("fake-sentinal");
        write_executable(&bin, "#!/bin/sh\necho 'boom' >&2\nexit 1\n");

        run_one_check(&ReleaseWatchSettings {
            data_dir: dir.clone(),
            sentinal_bin: bin,
        })
        .await; // must not panic

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn unparsable_output_does_not_panic() {
        let dir = scratch_dir("badjson");
        write_policy(&dir, "NOTIFY");
        let bin = dir.join("fake-sentinal");
        write_executable(&bin, "#!/bin/sh\necho 'not json'\nexit 0\n");

        run_one_check(&ReleaseWatchSettings {
            data_dir: dir.clone(),
            sentinal_bin: bin,
        })
        .await; // must not panic

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn jitter_stays_within_plus_minus_one_hour() {
        for seed in [
            0u32,
            1,
            100,
            3599,
            3600,
            3601,
            7199,
            7200,
            u32::MAX,
            123_456_789,
        ] {
            let duration = jittered_interval(seed);
            let secs = duration.as_secs();
            let bound =
                (BASE_INTERVAL_SECS - JITTER_RANGE_SECS)..=(BASE_INTERVAL_SECS + JITTER_RANGE_SECS);
            assert!(
                bound.contains(&secs),
                "seed {seed} produced {secs}s, outside the ±1h bound"
            );
        }
    }
}
