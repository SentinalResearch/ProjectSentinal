//! What the release watch does after each tick (4.E §11b.5): tell the owner
//! about a new signed release or an update result exactly once, and, under
//! "Automatic when idle", start the control update inside the maintenance
//! window when no live FULL Session would be interrupted. Pure decisions;
//! `release_watch` performs them.

use protocol::release::{ControlReleaseView, ControlUpdateState, ReleasePolicy};
use serde::{Deserialize, Serialize};
use std::path::Path;

const NOTIFIED_FILE: &str = "release-notified.json";

/// What has already been announced, stored owner-only beside the watch state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notified {
    /// The newest available version already announced.
    pub available_version: Option<String>,
    /// `finished_at` of the newest update result already announced.
    pub result_at: Option<i64>,
}

/// One thing the watch should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseAction {
    /// Send this release notice (title, body).
    Notice(String, String),
    /// Start the detached control update to this version.
    AutoUpdate(String),
}

/// Decide this tick's actions; `local_minutes` is the local time of day in
/// minutes, when known. Returns the actions and the new `Notified` state.
pub fn plan(
    view: &ControlReleaseView,
    notified: &Notified,
    local_minutes: Option<u32>,
) -> (Vec<ReleaseAction>, Notified) {
    let mut actions = Vec::new();
    let mut next = notified.clone();

    if let Some(update) = &view.update {
        let finished = update.finished_at.map(|at| at.0);
        let terminal = !matches!(update.state, ControlUpdateState::Running);
        if let (true, Some(at)) = (terminal, finished) {
            if notified.result_at.is_none_or(|seen| at > seen) {
                let (title, body) = match update.state {
                    ControlUpdateState::Updated => (
                        format!("Control updated to {}", update.version),
                        "Workers update from it when you choose.".to_string(),
                    ),
                    ControlUpdateState::RolledBack => (
                        format!("Control rolled back to {}", update.previous),
                        format!(
                            "{} failed its health check{}; automatic updates skip it.",
                            update.version,
                            update
                                .reason
                                .as_deref()
                                .map(|reason| format!(" ({reason})"))
                                .unwrap_or_default()
                        ),
                    ),
                    _ => (
                        format!("Control update to {} failed", update.version),
                        "The control kept its previous release.".to_string(),
                    ),
                };
                actions.push(ReleaseAction::Notice(title, body));
                next.result_at = Some(at);
            }
        }
    }

    let Some(offer) = view
        .available
        .as_ref()
        .filter(|_| view.policy != ReleasePolicy::Off)
    else {
        return (actions, next);
    };
    if notified.available_version.as_deref() != Some(offer.version.as_str()) {
        let body = match view.policy {
            ReleasePolicy::AutomaticWhenIdle => format!(
                "It installs between {} when this control is idle.",
                view.window
            ),
            _ => "Open Settings → Machines to update this control.".to_string(),
        };
        actions.push(ReleaseAction::Notice(
            format!("Sentinal {} is available", offer.version),
            body,
        ));
        next.available_version = Some(offer.version.clone());
    }

    let running = view
        .update
        .as_ref()
        .is_some_and(|update| update.state == ControlUpdateState::Running);
    // A version that already failed or rolled back is never retried automatically.
    let failed_before = view.update.as_ref().is_some_and(|update| {
        update.version == offer.version
            && matches!(
                update.state,
                ControlUpdateState::RolledBack
                    | ControlUpdateState::Failed
                    | ControlUpdateState::Unknown
            )
    });
    if view.policy == ReleasePolicy::AutomaticWhenIdle
        && !running
        && !failed_before
        && view.live_full_sessions == 0
        && local_minutes.is_some_and(|now| in_window(&view.window, now))
    {
        actions.push(ReleaseAction::AutoUpdate(offer.version.clone()));
    }
    (actions, next)
}

/// True when `now` (minutes since local midnight) is inside `HH:MM-HH:MM`,
/// which may wrap past midnight. A malformed window is never open.
pub fn in_window(window: &str, now: u32) -> bool {
    let parse = |text: &str| -> Option<u32> {
        let (hours, minutes) = text.split_once(':')?;
        let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
        (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
    };
    let Some((start, end)) = window
        .split_once('-')
        .and_then(|(start, end)| Some((parse(start)?, parse(end)?)))
    else {
        return false;
    };
    if start <= end {
        (start..end).contains(&now)
    } else {
        now >= start || now < end
    }
}

/// Read the announced state (missing or unreadable: nothing announced yet).
pub fn read_notified(data_dir: &Path) -> Notified {
    std::fs::read(data_dir.join(NOTIFIED_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Store the announced state owner-only.
pub fn write_notified(data_dir: &Path, notified: &Notified) -> std::io::Result<()> {
    use std::io::Write;
    let path = data_dir.join(NOTIFIED_FILE);
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = crate::private_fs::private_file_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    file.write_all(&serde_json::to_vec(notified).map_err(std::io::Error::other)?)?;
    std::fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::{in_window, plan, Notified, ReleaseAction};
    use protocol::{
        release::{
            AvailableRelease, ControlReleaseView, ControlUpdateState, ControlUpdateStatus,
            ReleaseChannel, ReleasePolicy,
        },
        Timestamp,
    };

    const THREE_AM: Option<u32> = Some(3 * 60 + 30);
    const NOON: Option<u32> = Some(12 * 60);

    fn view(policy: ReleasePolicy, available: Option<&str>) -> ControlReleaseView {
        ControlReleaseView {
            current: "0.1.17".into(),
            channel: ReleaseChannel::Stable,
            policy,
            window: "03:00-05:00".into(),
            available: available.map(|version| AvailableRelease {
                version: version.into(),
                published_at: String::new(),
                notes_url: String::new(),
            }),
            unsigned_found: None,
            last_checked_at: None,
            update: None,
            live_full_sessions: 0,
        }
    }

    fn update(
        state: ControlUpdateState,
        version: &str,
        finished_at: Option<i64>,
    ) -> ControlUpdateStatus {
        ControlUpdateStatus {
            update_id: "cu_1".into(),
            version: version.into(),
            previous: "0.1.17".into(),
            state,
            reason: Some("health check failed".into()),
            started_at: Some(Timestamp(1)),
            finished_at: finished_at.map(Timestamp),
        }
    }

    #[test]
    fn a_new_release_is_announced_once_and_never_auto_installed_under_notify() {
        let (actions, notified) = plan(
            &view(ReleasePolicy::Notify, Some("0.1.18")),
            &Notified::default(),
            THREE_AM,
        );
        assert_eq!(actions.len(), 1);
        assert!(
            matches!(&actions[0], ReleaseAction::Notice(title, _) if title == "Sentinal 0.1.18 is available")
        );
        let (again, _) = plan(
            &view(ReleasePolicy::Notify, Some("0.1.18")),
            &notified,
            THREE_AM,
        );
        assert!(again.is_empty());
        let (newer, _) = plan(
            &view(ReleasePolicy::Notify, Some("0.1.19")),
            &notified,
            THREE_AM,
        );
        assert_eq!(newer.len(), 1);
    }

    #[test]
    fn off_announces_no_release() {
        let (actions, _) = plan(
            &view(ReleasePolicy::Off, Some("0.1.18")),
            &Notified::default(),
            THREE_AM,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn automatic_updates_only_in_the_window_and_only_when_idle() {
        let announced = Notified {
            available_version: Some("0.1.18".into()),
            result_at: None,
        };
        let auto = view(ReleasePolicy::AutomaticWhenIdle, Some("0.1.18"));
        assert_eq!(
            plan(&auto, &announced, THREE_AM).0,
            vec![ReleaseAction::AutoUpdate("0.1.18".into())]
        );
        assert!(plan(&auto, &announced, NOON).0.is_empty());
        assert!(plan(&auto, &announced, None).0.is_empty());
        let busy = ControlReleaseView {
            live_full_sessions: 1,
            ..auto.clone()
        };
        assert!(plan(&busy, &announced, THREE_AM).0.is_empty());
        let running = ControlReleaseView {
            update: Some(update(ControlUpdateState::Running, "0.1.18", None)),
            ..auto.clone()
        };
        assert!(plan(&running, &announced, THREE_AM).0.is_empty());
    }

    #[test]
    fn a_rolled_back_version_is_announced_and_never_retried_automatically() {
        let auto = ControlReleaseView {
            update: Some(update(ControlUpdateState::RolledBack, "0.1.18", Some(500))),
            ..view(ReleasePolicy::AutomaticWhenIdle, Some("0.1.18"))
        };
        let announced = Notified {
            available_version: Some("0.1.18".into()),
            result_at: None,
        };
        let (actions, notified) = plan(&auto, &announced, THREE_AM);
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert!(matches!(&actions[0], ReleaseAction::Notice(title, body)
            if title == "Control rolled back to 0.1.17" && body.contains("health check failed")));
        assert_eq!(notified.result_at, Some(500));
        assert!(plan(&auto, &notified, THREE_AM).0.is_empty());
    }

    #[test]
    fn a_successful_update_is_announced_once() {
        let done = ControlReleaseView {
            update: Some(update(ControlUpdateState::Updated, "0.1.18", Some(900))),
            ..view(ReleasePolicy::Notify, None)
        };
        let (actions, notified) = plan(&done, &Notified::default(), NOON);
        assert!(
            matches!(&actions[..], [ReleaseAction::Notice(title, _)] if title == "Control updated to 0.1.18")
        );
        assert!(plan(&done, &notified, NOON).0.is_empty());
    }

    #[test]
    fn windows_may_wrap_midnight_and_malformed_windows_never_open() {
        assert!(in_window("03:00-05:00", 3 * 60));
        assert!(!in_window("03:00-05:00", 5 * 60));
        assert!(in_window("23:00-02:00", 23 * 60 + 30));
        assert!(in_window("23:00-02:00", 60));
        assert!(!in_window("23:00-02:00", 12 * 60));
        for bad in ["", "3-5", "25:00-26:00", "03:00"] {
            assert!(!in_window(bad, 3 * 60), "{bad}");
        }
    }
}
