//! Reclaiming keepers at startup (T5.2, `docs/R1_KEEPER_DESIGN.md` §9).
//!
//! For each live Session the store says a keeper holds: claim the keeper
//! (every check in §7, failing closed), cut the log back to its last closed
//! frame, rebuild the screen from the keeper's checkpoint plus the output
//! after it, and carry on: the output this daemon had not processed goes
//! through the log and the watcher as live output would. A keeper that
//! cannot be reclaimed is left alone, reported, and its Session interrupted
//! with the reason.

use super::kept::{offset_path, read_offset, Kept};
use super::{tasks, SessionManager};
use crate::keeper::client::{self, Claimed, KeeperError};
use crate::osc::OscParser;
use crate::tracker::CommandTracker;
use protocol::SessionId;

/// Reasons a Session ends at startup (§8).
pub const KEEPER_MISSING: &str =
    "Stopped: Sentinal restarted and could not find this terminal. The Machine may have restarted or logged out.";
/// The keeper did not match its record.
pub const KEEPER_MISMATCH: &str =
    "Stopped: Sentinal restarted and could not reclaim this terminal (its process did not match).";
/// The keeper speaks a protocol this daemon does not.
pub const KEEPER_VERSION: &str =
    "Stopped: this terminal was started by a Sentinal version that this one cannot reclaim.";

/// What happened to one keeper at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reclaim {
    /// The Session goes on; this many bytes were replayed.
    Kept {
        /// Output bytes the keeper replayed.
        replayed: usize,
        /// The screen started from the keeper's checkpoint.
        checkpoint: bool,
    },
    /// It could not be reclaimed; the Session was interrupted with this reason.
    Interrupted(&'static str),
}

impl SessionManager {
    /// Reclaim every keeper the store lists for a live Session.
    pub fn reclaim_keepers(&self) -> Vec<(SessionId, Reclaim)> {
        let Some(store) = self.inner.store.clone() else {
            return Vec::new();
        };
        let keepers = match store.live_keepers() {
            Ok(keepers) => keepers,
            Err(error) => {
                tracing::error!(%error, "could not list keepers to reclaim");
                return Vec::new();
            }
        };
        let mut results = Vec::with_capacity(keepers.len());
        for stored in keepers {
            let id = SessionId(stored.session_id);
            let outcome = match self.reclaim_one(&stored) {
                Ok((replayed, checkpoint)) => {
                    tracing::info!(
                        session = id.0,
                        replayed,
                        checkpoint,
                        "reclaimed a terminal after restart"
                    );
                    Reclaim::Kept {
                        replayed,
                        checkpoint,
                    }
                }
                Err(error) => {
                    let reason = match &error {
                        ReclaimError::Keeper(KeeperError::Missing(_)) => KEEPER_MISSING,
                        ReclaimError::Keeper(KeeperError::Version(_)) => KEEPER_VERSION,
                        _ => KEEPER_MISMATCH,
                    };
                    tracing::warn!(session = id.0, %error, "a terminal could not be reclaimed");
                    let _ = std::fs::remove_file(&stored.socket_path);
                    if let Err(error) =
                        store.interrupt_session(id, (self.inner.config.clock)(), reason)
                    {
                        tracing::error!(session = id.0, %error, "could not record the interruption");
                    }
                    Reclaim::Interrupted(reason)
                }
            };
            results.push((id, outcome));
        }
        results
    }

    fn reclaim_one(&self, stored: &store::StoredKeeper) -> Result<(usize, bool), ReclaimError> {
        let store = self.inner.store.clone().ok_or(ReclaimError::NoStore)?;
        let id = SessionId(stored.session_id);
        let session = store.session(id)?.ok_or(ReclaimError::NoSession)?;
        let record = crate::keeper::from_stored(stored);
        let log_path = self
            .inner
            .config
            .data_dir
            .join("sessions")
            .join(format!("{}.log.zst", id.0));
        let offsets = offset_path(&log_path);
        let (resume_from, log_length) = read_offset(&offsets);
        let claimed = client::claim(&record, resume_from)?;

        // The output this daemon processed before it went is only redrawn;
        // what came after is new and goes through everything.
        let screen_only =
            (resume_from.saturating_sub(claimed.replay_from) as usize).min(claimed.replay.len());
        let (redraw, fresh) = claimed.replay.split_at(screen_only);
        let replayed = claimed.replay.len();
        let fresh_from = claimed.replay_from + screen_only as u64;

        // Cut the log back to its last closed frame, then append.
        let log_file = {
            let file = std::fs::OpenOptions::new().write(true).open(&log_path)?;
            if file.metadata()?.len() > log_length {
                file.set_len(log_length)?;
            }
            crate::private_fs::private_file_options()
                .append(true)
                .open(&log_path)?
        };

        let mut marker = Vec::new();
        if let Some(gap) = claimed.gap.filter(|gap| gap.to > resume_from) {
            marker = format!(
                "\r\n[sentinal: {} bytes of output were not recorded while sentinald was restarting]\r\n",
                gap.to - gap.from.max(resume_from)
            )
            .into_bytes();
        }
        let mut first = marker;
        first.extend_from_slice(fresh);
        let spawned = client::into_spawned(id, record.clone(), &claimed, first)?;
        let current = session.current_command.and_then(|command_id| {
            store
                .history(Some(id), 20)
                .ok()?
                .into_iter()
                .find(|command| command.id == command_id && command.finished_at.is_none())
        });
        let mut session = session;
        session.kept = true;
        let crate::pty::SpawnedPty {
            root_pid,
            reader,
            writer,
            resize,
            kill,
            try_wait,
            keeper,
        } = spawned;
        let handle = self.build_handle(super::lifecycle::HandleParts {
            id,
            root_pid,
            session: session.clone(),
            writer,
            resize,
            kill,
            try_wait,
            size: claimed.claimed.size,
            tracker: CommandTracker::resume(
                self.inner.config.machine_id.clone(),
                session.cwd.clone(),
                self.inner.tracker_ids.clone(),
                current,
            ),
            initial_command: None,
        });
        rebuild_screen(&handle.screen, &claimed, redraw, &record.osc_nonce);
        if claimed.gap.is_some() {
            // Bytes are missing from the screen: a full-screen program redraws
            // on a resize to the same size.
            let (cols, rows) = claimed.claimed.size;
            let _ = (handle.resize)(cols, rows.saturating_sub(1).max(1));
            let _ = (handle.resize)(cols, rows);
        }
        {
            let mut sessions = self
                .inner
                .sessions
                .write()
                .map_err(|_| ReclaimError::Lock)?;
            sessions.insert(id, handle.clone());
        }
        let _ = store.update_session(&session);
        let kept = keeper.map(|keeper| Kept::new(keeper.checkpoint, offsets, fresh_from));
        let try_wait = handle.try_wait.clone();
        tasks::spawn_reader(
            id,
            reader,
            handle.clone(),
            tasks::OutputLog {
                path: log_path,
                file: log_file,
                kept,
            },
            OscParser::new(record.osc_nonce.clone()),
            Some(store.clone()),
            self.inner.config.clock.clone(),
        );
        tasks::spawn_exit_monitor(
            id,
            handle,
            try_wait,
            self.inner.config.clock.clone(),
            Some(store),
        );
        Ok((replayed, claimed.checkpoint.is_some()))
    }
}

/// The checkpoint, then the output after it up to where this daemon had
/// processed, through the same OSC and sequence filters as live output.
fn rebuild_screen(screen: &crate::screen::Screen, claimed: &Claimed, redraw: &[u8], nonce: &str) {
    if let Some((_, snapshot)) = &claimed.checkpoint {
        if !screen.restore(snapshot.clone()) {
            tracing::warn!(
                "a checkpoint could not be read; the screen starts from the kept output"
            );
        }
    }
    let mut osc = OscParser::new(nonce.to_string());
    let mut boundary = super::sequence_boundary::SequenceBoundary::default();
    let parsed = osc.push(redraw);
    let mut bytes = boundary.push(&parsed.output);
    bytes.extend(boundary.push(&osc.finish()));
    bytes.extend(boundary.finish());
    screen.feed(bytes);
}

/// Why one keeper was not reclaimed.
#[derive(Debug, thiserror::Error)]
enum ReclaimError {
    #[error(transparent)]
    Keeper(#[from] KeeperError),
    #[error("the Session has no stored record")]
    NoSession,
    #[error("no store")]
    NoStore,
    #[error("the session map lock is poisoned")]
    Lock,
    #[error(transparent)]
    Store(#[from] store::StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
