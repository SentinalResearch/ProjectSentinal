//! The single error type for this crate.

use crate::{CaptureLevel, CommandId, SessionOrigin, SessionState};

/// Everything that can go wrong when using protocol types.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProtocolError {
    /// A session was asked to move to a state it cannot reach from its current one.
    #[error("session cannot go from {from:?} to {to:?}")]
    InvalidTransition {
        /// State before the attempted change.
        from: SessionState,
        /// State that was requested.
        to: SessionState,
    },

    /// `finish` was called on a command that already has an exit code.
    #[error("command {0:?} is already finished")]
    CommandAlreadyFinished(CommandId),

    /// A command's finish time is earlier than its start time.
    #[error("command finish time {finished_at} is before start time {started_at}")]
    FinishedBeforeStart {
        /// When the command started (Unix ms).
        started_at: i64,
        /// The invalid finish time (Unix ms).
        finished_at: i64,
    },

    /// A recipe template uses `{name}` but no parameter called `name` is declared.
    #[error("recipe template uses '{{{0}}}' but no parameter with that name is declared")]
    UndeclaredPlaceholder(String),

    /// A value was given for a parameter the recipe does not declare.
    #[error("unknown recipe parameter '{0}'")]
    UnknownParam(String),

    /// A parameter has no default and no value was given.
    #[error("recipe parameter '{0}' has no default and no value was given")]
    MissingParam(String),

    /// A recipe template has a `{` without a matching `}` (use `{{` for a literal brace).
    #[error("recipe template has an unclosed '{{' at byte {0}")]
    UnclosedBrace(usize),

    /// A message was sent with a protocol version we do not understand.
    #[error("unsupported protocol version {got} (this build supports {supported})")]
    UnsupportedVersion {
        /// Version found in the message.
        got: u32,
        /// Version this build speaks.
        supported: u32,
    },

    /// A terminal size of zero columns or rows was requested.
    #[error("terminal size must be at least 1x1, got {cols}x{rows}")]
    InvalidTerminalSize {
        /// Requested columns.
        cols: u16,
        /// Requested rows.
        rows: u16,
    },

    /// A process identifier cannot name an OS process.
    #[error("invalid pid {0}; pid must be greater than zero")]
    InvalidPid(u32),

    /// A local shell sequence cannot identify a command.
    #[error("invalid command sequence {0}; sequence must be greater than zero")]
    InvalidCommandSequence(u64),

    /// A typed identifier used the reserved zero value.
    #[error("invalid {field} 0; {field} must be greater than zero")]
    InvalidIdentifier {
        /// Input field containing the identifier.
        field: &'static str,
    },

    /// A required string contained no bytes.
    #[error("{field} must not be empty")]
    EmptyField {
        /// Input field that was empty.
        field: &'static str,
    },

    /// A bounded string exceeded its protocol limit.
    #[error("{field} is {actual} bytes; maximum is {maximum} bytes")]
    FieldTooLong {
        /// Input field that was too long.
        field: &'static str,
        /// Actual UTF-8 byte count.
        actual: usize,
        /// Maximum permitted byte count.
        maximum: usize,
    },

    /// Initial structured Command argv contained no executable.
    #[error("command argv must contain an executable")]
    EmptyCommandArgv,

    /// Initial structured Command argv used an empty executable.
    #[error("command executable argv[0] must not be empty")]
    EmptyCommandExecutable,

    /// Combined structured Command arguments exceeded the wire limit.
    #[error("command argv is {actual} bytes; maximum is {maximum} bytes")]
    CommandArgumentsTooLong {
        /// Combined argument byte count.
        actual: usize,
        /// Maximum permitted byte count.
        maximum: usize,
    },

    /// A Session origin was paired with a capture level it cannot provide.
    #[error("illegal origin/capture combination {origin}/{capture}")]
    InvalidOriginCapture {
        /// Session origin wire name.
        origin: SessionOrigin,
        /// Capture level wire name.
        capture: CaptureLevel,
    },

    /// OS process start time was supplied without a root PID.
    #[error("process_started_at requires a process-tree root pid")]
    ProcessStartedWithoutPid,

    /// A Session exit code contradicted its lifecycle state.
    #[error("exit_code {exit_code} is invalid for session state {state:?}")]
    InvalidSessionExit {
        /// Current lifecycle state.
        state: SessionState,
        /// Exit code that was supplied.
        exit_code: i32,
    },

    /// The JSON could not be parsed into the expected type.
    #[error("invalid JSON: {0}")]
    Json(String),
}

impl From<serde_json::Error> for ProtocolError {
    fn from(err: serde_json::Error) -> Self {
        ProtocolError::Json(err.to_string())
    }
}
