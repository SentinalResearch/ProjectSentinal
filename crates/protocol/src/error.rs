//! The single error type for this crate.

use crate::{CaptureLevel, CommandId, SessionId, SessionOrigin, SessionState};

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

    /// A Recipe value contained terminal control bytes.
    #[error("recipe parameter '{0}' contains terminal control characters")]
    InvalidRecipeParamValue(String),

    /// A Recipe placeholder was embedded in shell syntax where quoting cannot isolate it.
    #[error("recipe placeholder '{{{0}}}' must be a standalone, unquoted shell word")]
    InvalidRecipePlaceholderContext(String),

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

    /// Only adopted Sessions may carry log metadata.
    #[error("log metadata is only valid for ADOPTED Sessions, not {origin}")]
    LogWithoutAdoption {
        /// Session origin.
        origin: SessionOrigin,
    },
    /// A Session log path must be absolute.
    #[error("log path '{0}' must be absolute")]
    RelativeLogPath(String),

    /// A Session named itself as its direct predecessor.
    #[error("Session {0} cannot continue itself")]
    SelfContinuation(SessionId),

    /// A Health progress percentage was not finite or outside 0–100.
    #[error("health progress {0} must be finite and between 0 and 100")]
    InvalidHealthProgress(f32),

    /// A Health snapshot contained too many evidence entries.
    #[error("health evidence has {actual} entries; maximum is {maximum}")]
    TooManyHealthEvidence {
        /// Evidence entries supplied.
        actual: usize,
        /// Maximum accepted entries.
        maximum: usize,
    },

    /// A bounded request limit was zero or too large.
    #[error("{field} limit {actual} must be between 1 and {maximum}")]
    InvalidQueryLimit {
        /// Query field containing the limit.
        field: &'static str,
        /// Rejected limit.
        actual: u32,
        /// Maximum supported limit.
        maximum: u32,
    },

    /// A Machine display name broke the naming rules.
    #[error("display_name {reason}")]
    InvalidDisplayName {
        /// Which rule failed (never repeats rejected control characters).
        reason: &'static str,
    },

    /// A pairing code had the wrong shape; the code itself is never echoed.
    #[error("pairing code must be 10 characters from the Sentinal code alphabet")]
    InvalidPairingCode,

    /// A control URL was not an accepted origin.
    #[error("control URL {reason}")]
    InvalidControlUrl {
        /// Which rule failed.
        reason: &'static str,
    },

    /// A native-notification HTTP field failed its shape or value rules.
    #[error("{field} {reason}")]
    InvalidNotificationField {
        /// Field containing the rejected value.
        field: &'static str,
        /// Stable reason that never repeats a secret value.
        reason: &'static str,
    },

    /// A control projection field failed compatibility or bounds validation.
    #[error("projection {field} {reason}")]
    InvalidProjectionField {
        /// Projection field containing the bad input.
        field: &'static str,
        /// Stable reason that does not repeat untrusted content.
        reason: &'static str,
    },

    /// The JSON could not be parsed into the expected type.
    #[error("invalid JSON: {0}")]
    Json(String),
    /// A release version was not strict `MAJOR.MINOR.PATCH`.
    #[error("invalid release version '{0}'; expected MAJOR.MINOR.PATCH")]
    InvalidReleaseVersion(String),
    /// A discovery response was not a compatible control for the probed origin.
    #[error("invalid discovery response: {0}")]
    InvalidDiscovery(&'static str),
    /// A pairing request had the wrong shape.
    #[error("invalid pairing request: {0}")]
    InvalidPairingRequest(&'static str),
}

impl From<serde_json::Error> for ProtocolError {
    fn from(err: serde_json::Error) -> Self {
        ProtocolError::Json(err.to_string())
    }
}
