//! Errors produced by the Session manager.

use crate::pty::PtyError;
use protocol::{CaptureLevel, ProtocolError, SessionId, SessionOrigin};
use std::path::PathBuf;
use store::StoreError;

/// Errors produced while managing sessions.
#[derive(Debug, thiserror::Error)]
pub enum SessionManagerError {
    /// A session ID is not owned by this daemon.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    /// A terminal dimension was zero.
    #[error("terminal size must be at least 1x1, got {cols}x{rows}")]
    InvalidTerminalSize {
        /// Requested columns.
        cols: u16,
        /// Requested rows.
        rows: u16,
    },
    /// A PTY operation failed.
    #[error(transparent)]
    Pty(#[from] PtyError),
    /// A session state transition failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// A session data file operation failed.
    #[error("failed to {operation} session data '{}': {source}", path.display())]
    File {
        /// Operation that failed.
        operation: &'static str,
        /// Data path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Shared session state was poisoned by a panic.
    #[error("failed to lock {resource} for session {session_id}: lock poisoned")]
    Lock {
        /// Resource being accessed.
        resource: &'static str,
        /// Session being accessed.
        session_id: SessionId,
    },
    /// A remote input audit record could not be written.
    #[error(transparent)]
    Audit(#[from] crate::audit::AuditError),
    /// Structured state could not be persisted.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// This manager was constructed without persistent storage.
    #[error("persistent command history is unavailable")]
    StoreUnavailable,
    /// A capture level does not permit a PTY operation.
    #[error("capture level {capture} cannot {operation}")]
    CaptureDenied {
        /// Capture level of the target Session.
        capture: CaptureLevel,
        /// Requested operation.
        operation: &'static str,
    },
    /// An origin that CREATE_SESSION cannot create yet was requested.
    #[error("Session origin {origin} cannot be created through CREATE_SESSION")]
    OriginUnavailable {
        /// Requested Session origin.
        origin: SessionOrigin,
    },
    /// An initial Command was supplied for a non-Watch Session.
    #[error("an initial Command is only accepted for WATCH Sessions, not {origin}")]
    InitialCommandRequiresWatch {
        /// Requested Session origin.
        origin: SessionOrigin,
    },
    /// The shell cannot report readiness, so an initial Command cannot run safely.
    #[error(
        "an initial Command needs zsh, bash, or fish with Sentinel hooks; shell '{shell}' is unsupported"
    )]
    InitialCommandShell {
        /// Requested shell executable.
        shell: String,
    },
    /// Shell hooks could not be installed, so readiness would never be reported.
    #[error("an initial Command needs Sentinel shell hooks, which failed to install: {0}")]
    InitialCommandHooks(String),
}
