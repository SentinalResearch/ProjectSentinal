//! Messages that travel over the WebSocket between clients and the daemon.
//!
//! Every JSON message is wrapped in an [`Envelope`] with a protocol version.
//! Raw terminal bytes do **not** use these types; they go in binary frames.

use crate::{
    link::{
        normalize_pairing_code, validate_control_url, validate_display_name, ConnectionReport,
        ControlUrlPolicy, Secret,
    },
    Command, Event, Machine, MachineId, MachineMetrics, NotificationJournalEntry,
    NotificationRuleSet, NotificationRules, NotificationStatus, ProtocolError, Recipe, Session,
    SessionId, SessionMetrics, SessionOrigin, Timestamp,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;

/// Current protocol version. Bump when a change breaks old clients.
pub const PROTOCOL_VERSION: u32 = 1;
/// Maximum JSON body accepted on the private local socket.
pub const MAX_LOCAL_FRAME_BYTES: usize = 65_536;
/// Maximum UTF-8 bytes in externally reported Command text.
pub const MAX_COMMAND_TEXT_BYTES: usize = 49_152;
const MAX_TITLE_BYTES: usize = 256;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_ARGUMENT_BYTES: usize = 16_384;
const MAX_ARGUMENTS_BYTES: usize = 49_152;

/// A versioned wrapper around any message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Envelope<T> {
    /// Protocol version the sender speaks.
    pub version: u32,
    /// The message itself.
    pub payload: T,
}

impl<T: Serialize + DeserializeOwned> Envelope<T> {
    /// Wrap a message with the current protocol version.
    pub fn new(payload: T) -> Self {
        Envelope {
            version: PROTOCOL_VERSION,
            payload,
        }
    }

    /// Serialize to a JSON string.
    pub fn encode(&self) -> Result<String, ProtocolError> {
        Ok(serde_json::to_string(self)?)
    }

    /// Parse JSON, rejecting versions this build does not speak.
    ///
    /// The version is checked before the payload, so a newer client gets a
    /// clear "unsupported version" error instead of a confusing parse error.
    pub fn decode(json: &str) -> Result<Self, ProtocolError> {
        #[derive(Deserialize)]
        struct VersionOnly {
            version: u32,
        }
        let VersionOnly { version } = serde_json::from_str(json)?;
        if version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion {
                got: version,
                supported: PROTOCOL_VERSION,
            });
        }
        Ok(serde_json::from_str(json)?)
    }
}

/// Requests a client (web UI, CLI, agent) sends to the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientRequest {
    /// Ask for the machine description.
    GetMachine,
    /// Ask for all sessions on this machine.
    ListSessions,
    /// Start a new session.
    CreateSession {
        /// Shell to run; `None` uses the user's default shell.
        shell: Option<String>,
        /// Starting directory; `None` uses the home directory.
        cwd: Option<String>,
        /// Terminal width in columns.
        cols: u16,
        /// Terminal height in rows.
        rows: u16,
        /// How this daemon-owned Session was requested.
        #[serde(default)]
        origin: SessionOrigin,
        /// Optional user-visible Session title.
        #[serde(default)]
        title: Option<String>,
        /// Optional initial Command represented as executable plus exact arguments.
        #[serde(default)]
        command: Option<Vec<String>>,
    },
    /// Continue one finished FULL Session as a distinct generation.
    ContinueSession {
        /// Finished source Session.
        session_id: SessionId,
    },
    /// Start streaming a session's output to this client.
    Attach {
        /// Session to attach to.
        session_id: SessionId,
    },
    /// Stop streaming a session's output. The session keeps running.
    Detach {
        /// Session to detach from.
        session_id: SessionId,
    },
    /// Change the terminal size.
    Resize {
        /// Session to resize.
        session_id: SessionId,
        /// New width in columns.
        cols: u16,
        /// New height in rows.
        rows: u16,
    },
    /// Kill the session's shell and everything in it.
    Kill {
        /// Session to kill.
        session_id: SessionId,
    },
    /// Query stored command history, optionally restricted to one session.
    GetHistory {
        /// Session to restrict results to; `None` returns machine-wide history.
        session_id: Option<SessionId>,
        /// Maximum number of commands to return.
        limit: u32,
    },
    /// Query persisted Events, optionally restricted to one Session.
    GetEvents {
        /// Session to restrict results to; `None` returns machine-wide Events.
        session_id: Option<SessionId>,
        /// Return Events with IDs greater than this value.
        after: Option<crate::EventId>,
        /// Maximum number of Events, from 1 through 500.
        limit: u32,
    },
    /// Search command text using the daemon's full-text index.
    SearchCommands {
        /// Full-text query entered by the user.
        query: String,
        /// Maximum number of commands to return.
        limit: u32,
    },
    /// List Recipes available on this Machine.
    ListRecipes,
    /// Render and run a Recipe on this Machine.
    RunRecipe {
        /// Daemon-local Recipe ID.
        recipe_id: String,
        /// User values rendered through [`Recipe::render`].
        values: BTreeMap<String, String>,
        /// Existing Session to receive the command, or a new Session when absent.
        session_id: Option<SessionId>,
    },
    /// Validate and store a user Recipe on this Machine.
    SaveRecipe {
        /// Recipe definition to persist.
        recipe: Recipe,
    },
    /// Read default and per-Session notification rules.
    GetNotificationRules,
    /// Replace the default rule or one Session override.
    SetNotificationRule {
        /// `None` changes this Machine's default.
        session_id: Option<SessionId>,
        /// Complete rule value.
        rules: NotificationRuleSet,
    },
    /// Read the bounded, secret-free delivery journal.
    GetNotificationJournal,
    /// Host, daemon, link, and per-Session process facts for the Machine page.
    GetMachineDetails,
    /// Machine-wide CPU, RAM, and GPU history over `range`.
    GetMachineMetrics {
        /// How far back to reach.
        range: crate::MetricRange,
    },
}

impl ClientRequest {
    /// Check values that the type system cannot, like a zero terminal size.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            ClientRequest::CreateSession { cols, rows, .. }
            | ClientRequest::Resize { cols, rows, .. }
                if *cols == 0 || *rows == 0 =>
            {
                Err(ProtocolError::InvalidTerminalSize {
                    cols: *cols,
                    rows: *rows,
                })
            }
            ClientRequest::CreateSession {
                shell,
                cwd,
                origin,
                title,
                command,
                ..
            } => {
                if !matches!(
                    origin,
                    SessionOrigin::Ui | SessionOrigin::Watch | SessionOrigin::AutoWrap
                ) {
                    return Err(ProtocolError::InvalidOriginCapture {
                        origin: *origin,
                        capture: crate::CaptureLevel::Full,
                    });
                }
                validate_optional_text("shell", shell.as_deref(), MAX_PATH_BYTES)?;
                validate_optional_text("cwd", cwd.as_deref(), MAX_PATH_BYTES)?;
                validate_optional_text("title", title.as_deref(), MAX_TITLE_BYTES)?;
                if let Some(argv) = command {
                    validate_argv(argv)?;
                }
                Ok(())
            }
            ClientRequest::ContinueSession { session_id } if session_id.0 == 0 => {
                Err(ProtocolError::InvalidIdentifier {
                    field: "session_id",
                })
            }
            ClientRequest::GetEvents {
                session_id: Some(session_id),
                ..
            } if session_id.0 == 0 => Err(ProtocolError::InvalidIdentifier {
                field: "session_id",
            }),
            ClientRequest::GetEvents { limit, .. } if !(1..=500).contains(limit) => {
                Err(ProtocolError::InvalidQueryLimit {
                    field: "events",
                    actual: *limit,
                    maximum: 500,
                })
            }
            _ => Ok(()),
        }
    }
}

/// Requests accepted only through the same-user private Unix socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LocalRequest {
    /// Register an ordinary interactive shell for COMMANDS capture.
    RegisterShell {
        /// Root shell PID.
        pid: u32,
        /// Shell executable.
        shell: String,
        /// Current working directory.
        cwd: String,
        /// Optional display title.
        title: Option<String>,
    },
    /// Record an exact external-shell Command start.
    ShellCommandStarted {
        /// Registered shell Session.
        session_id: SessionId,
        /// Registered shell PID.
        pid: u32,
        /// Monotonic per-shell correlation sequence.
        sequence: u64,
        /// Exact Command text reported by the shell.
        text: String,
        /// Working directory at Command start.
        cwd: String,
        /// Shell-observed start timestamp.
        started_at: Timestamp,
    },
    /// Finish the external-shell Command with the matching sequence.
    ShellCommandFinished {
        /// Registered shell Session.
        session_id: SessionId,
        /// Registered shell PID.
        pid: u32,
        /// Sequence supplied at Command start.
        sequence: u64,
        /// Shell exit code.
        exit_code: i32,
        /// Shell-observed finish timestamp.
        finished_at: Timestamp,
    },
    /// Register an existing same-user process for read-only observation.
    AdoptProcess {
        /// Process-tree root PID.
        pid: u32,
        /// Optional display title.
        title: Option<String>,
        /// Optional explicit read-only log path.
        log_path: Option<String>,
    },
    /// Return this daemon's immutable Machine ID.
    GetIdentity,
    /// Pair this daemon with a control Machine using a one-time code.
    ConnectControl {
        /// Control origin, e.g. `https://sentinal-control.example.ts.net`.
        control_url: String,
        /// One-time code typed by the owner; never logged.
        pairing_code: Secret,
        /// Proposed display name; the hostname when absent.
        display_name: Option<String>,
    },
    /// Report this daemon's control link.
    ConnectionStatus,
    /// Remove this daemon's control link and tell the control when possible.
    DisconnectControl,
    /// Replace a corrupt or unwanted Machine identity with a new one.
    ResetIdentity,
    /// Ask a discovered control to approve this Machine (4.E §6).
    RequestApproval {
        /// Control origin.
        control_url: String,
        /// The control's Machine ID from discovery; a different answer is refused.
        control_machine_id: MachineId,
        /// Proposed display name; the hostname when absent.
        display_name: Option<String>,
        /// Moving from another control: keep the current link until this
        /// request is claimed (absent from older CLIs: false).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        keep_current_link: bool,
    },
    /// Try to claim the pending approval once.
    ClaimApproval,
    /// Withdraw the pending approval request.
    CancelApproval,
    /// On a control: list (`dry_run`) or move every active worker to another control.
    HandoffWorkers {
        /// The control to move them to; `None` only lists them (`dry_run`).
        target: Option<crate::pairing::LinkMoveRequest>,
        /// Only list the workers that would be moved.
        dry_run: bool,
    },
}

impl LocalRequest {
    /// Validate fields before local dispatch mutates structured state.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            LocalRequest::RegisterShell {
                pid,
                shell,
                cwd,
                title,
            } => {
                validate_pid(*pid)?;
                validate_text("shell", shell, MAX_PATH_BYTES)?;
                validate_text("cwd", cwd, MAX_PATH_BYTES)?;
                validate_optional_text("title", title.as_deref(), MAX_TITLE_BYTES)
            }
            LocalRequest::ShellCommandStarted {
                session_id,
                pid,
                sequence,
                text,
                cwd,
                ..
            } => {
                validate_local_identity(*session_id, *pid, *sequence)?;
                validate_text("command text", text, MAX_COMMAND_TEXT_BYTES)?;
                validate_text("cwd", cwd, MAX_PATH_BYTES)
            }
            LocalRequest::ShellCommandFinished {
                session_id,
                pid,
                sequence,
                ..
            } => validate_local_identity(*session_id, *pid, *sequence),
            LocalRequest::AdoptProcess {
                pid,
                title,
                log_path,
            } => {
                validate_pid(*pid)?;
                validate_optional_text("title", title.as_deref(), MAX_TITLE_BYTES)?;
                validate_optional_text("log_path", log_path.as_deref(), MAX_PATH_BYTES)
            }
            LocalRequest::ConnectControl {
                control_url,
                pairing_code,
                display_name,
            } => {
                validate_control_url(control_url, ControlUrlPolicy::Development)
                    .or_else(|_| validate_control_url(control_url, ControlUrlPolicy::Production))?;
                normalize_pairing_code(pairing_code.expose())?;
                display_name
                    .as_deref()
                    .map(validate_display_name)
                    .transpose()?;
                Ok(())
            }
            LocalRequest::RequestApproval {
                control_url,
                control_machine_id,
                display_name,
                ..
            } => {
                validate_control_url(control_url, ControlUrlPolicy::Development)
                    .or_else(|_| validate_control_url(control_url, ControlUrlPolicy::Production))?;
                if !control_machine_id.is_ulid() {
                    return Err(ProtocolError::InvalidPairingRequest(
                        "control Machine ID is not a ULID",
                    ));
                }
                display_name
                    .as_deref()
                    .map(validate_display_name)
                    .transpose()?;
                Ok(())
            }
            LocalRequest::ClaimApproval | LocalRequest::CancelApproval => Ok(()),
            LocalRequest::HandoffWorkers { target, dry_run } => match target {
                Some(target) => target.validate(ControlUrlPolicy::Development),
                None if *dry_run => Ok(()),
                None => Err(ProtocolError::InvalidPairingRequest(
                    "a handoff needs a control to move the workers to",
                )),
            },
            LocalRequest::GetIdentity
            | LocalRequest::ConnectionStatus
            | LocalRequest::DisconnectControl
            | LocalRequest::ResetIdentity => Ok(()),
        }
    }
}

/// Stable categories for private local-ingress errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LocalErrorCode {
    /// JSON or field validation failed.
    InvalidRequest,
    /// Peer identity did not authorize the request.
    Forbidden,
    /// Referenced process or Session was not found.
    NotFound,
    /// Existing state conflicts with the request.
    Conflict,
    /// The operation belongs to a later Milestone 1b stage.
    Unavailable,
    /// An internal operation failed safely.
    Internal,
    /// The control rejected the pairing code, or pairing is locked.
    CodeRejected,
    /// The control could not be reached or is not a control.
    ControlUnreachable,
    /// This daemon already has a control link.
    AlreadyConnected,
    /// This daemon has no control link.
    NotConnected,
    /// The Machine identity file is unreadable or invalid.
    IdentityCorrupt,
}

/// Replies emitted by the private local Unix socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LocalResponse {
    /// A local Session was registered or found.
    Session {
        /// Current Session state.
        session: Session,
    },
    /// A local Command was recorded or updated.
    Command {
        /// Current Command state.
        command: Command,
    },
    /// This daemon's Machine identity.
    Identity {
        /// Immutable Machine ID.
        machine_id: MachineId,
        /// Hostname, the default display name.
        hostname: String,
    },
    /// This daemon's control link.
    Connection {
        /// Link state without secrets.
        connection: ConnectionReport,
    },
    /// The control recorded and verified an approval request.
    ApprovalRequested {
        /// `pr_` handle.
        request_id: String,
        /// Words the owner must see on the control too.
        verification_words: Vec<String>,
        /// When the request expires.
        expires_at: Timestamp,
    },
    /// The approval request has not produced a link (yet, or ever).
    ApprovalState {
        /// `PENDING` (ask again), `REJECTED`, `CANCELLED`, or `EXPIRED`.
        state: crate::pairing::PairingRequestState,
    },
    /// Workers listed or moved by a handoff.
    Handoff {
        /// One entry per active worker.
        workers: Vec<crate::pairing::WorkerHandoff>,
    },
    /// Request failed without stopping the listener.
    Error {
        /// Stable error category.
        code: LocalErrorCode,
        /// Bounded human-readable description.
        message: String,
    },
}

fn validate_pid(pid: u32) -> Result<(), ProtocolError> {
    if pid == 0 {
        Err(ProtocolError::InvalidPid(pid))
    } else {
        Ok(())
    }
}

fn validate_local_identity(
    session_id: SessionId,
    pid: u32,
    sequence: u64,
) -> Result<(), ProtocolError> {
    if session_id.0 == 0 {
        return Err(ProtocolError::InvalidIdentifier {
            field: "session_id",
        });
    }
    validate_pid(pid)?;
    if sequence == 0 {
        return Err(ProtocolError::InvalidCommandSequence(sequence));
    }
    Ok(())
}

pub(crate) fn validate_argv(argv: &[String]) -> Result<(), ProtocolError> {
    let Some(executable) = argv.first() else {
        return Err(ProtocolError::EmptyCommandArgv);
    };
    if executable.is_empty() {
        return Err(ProtocolError::EmptyCommandExecutable);
    }
    let mut total = 0_usize;
    for argument in argv {
        if argument.len() > MAX_ARGUMENT_BYTES {
            return Err(ProtocolError::FieldTooLong {
                field: "command argument",
                actual: argument.len(),
                maximum: MAX_ARGUMENT_BYTES,
            });
        }
        total = total.saturating_add(argument.len());
    }
    if total > MAX_ARGUMENTS_BYTES {
        return Err(ProtocolError::CommandArgumentsTooLong {
            actual: total,
            maximum: MAX_ARGUMENTS_BYTES,
        });
    }
    Ok(())
}

fn validate_optional_text(
    field: &'static str,
    value: Option<&str>,
    maximum: usize,
) -> Result<(), ProtocolError> {
    value.map_or(Ok(()), |value| validate_text(field, value, maximum))
}

fn validate_text(field: &'static str, value: &str, maximum: usize) -> Result<(), ProtocolError> {
    if value.is_empty() {
        return Err(ProtocolError::EmptyField { field });
    }
    if value.len() > maximum {
        return Err(ProtocolError::FieldTooLong {
            field,
            actual: value.len(),
            maximum,
        });
    }
    Ok(())
}

/// Messages the daemon sends to clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DaemonMessage {
    /// Description of this machine.
    Machine {
        /// The machine.
        machine: Machine,
    },
    /// Full list of sessions (reply to `LIST_SESSIONS`).
    Sessions {
        /// All sessions on this machine.
        sessions: Vec<Session>,
    },
    /// A session was created or changed.
    SessionUpdated {
        /// The session's new state.
        session: Session,
    },
    /// A command started or finished.
    CommandUpdated {
        /// The command's new state.
        command: Command,
    },
    /// Something happened.
    Event {
        /// The event.
        event: Event,
    },
    /// Bounded persisted Events ordered oldest first.
    Events {
        /// Events matching the request.
        events: Vec<Event>,
    },
    /// Current resource measurements for the machine.
    MachineMetrics {
        /// The machine measurements.
        metrics: MachineMetrics,
    },
    /// Current aggregate resource measurements for a session.
    SessionMetrics {
        /// The session measurements.
        metrics: SessionMetrics,
    },
    /// Stored commands ordered from newest to oldest.
    History {
        /// Commands matching the history request.
        commands: Vec<Command>,
    },
    /// Recipes available on this Machine.
    Recipes {
        /// Valid built-in and user Recipes.
        recipes: Vec<Recipe>,
    },
    /// Current notification rules and safe delivery configuration.
    NotificationRules {
        /// Rules stored by this Machine.
        rules: NotificationRules,
        /// Secret-free ntfy configuration state.
        status: NotificationStatus,
    },
    /// Reply to `GET_MACHINE_DETAILS`.
    MachineDetails {
        /// Facts for the Machine page.
        details: crate::MachineDetails,
    },
    /// Reply to `GET_MACHINE_METRICS`, oldest point first.
    MachineMetricsHistory {
        /// The requested range.
        range: crate::MetricRange,
        /// Bucketed averages.
        points: Vec<crate::MachineMetricPoint>,
    },
    /// Bounded notification delivery journal.
    NotificationJournal {
        /// Newest attempted deliveries first.
        entries: Vec<NotificationJournalEntry>,
    },
    /// A request failed.
    Error {
        /// Human-readable reason.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trips_through_json() {
        let original = Envelope::new(ClientRequest::ListSessions);
        let decoded = Envelope::<ClientRequest>::decode(&original.encode().unwrap()).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn decode_rejects_other_protocol_versions() {
        let json = r#"{"version": 999, "payload": {"type": "LIST_SESSIONS"}}"#;
        assert_eq!(
            Envelope::<ClientRequest>::decode(json),
            Err(ProtocolError::UnsupportedVersion {
                got: 999,
                supported: PROTOCOL_VERSION
            })
        );
    }

    #[test]
    fn decode_reports_version_error_even_if_payload_is_unknown() {
        let json = r#"{"version": 2, "payload": {"type": "SOMETHING_NEW"}}"#;
        assert!(matches!(
            Envelope::<ClientRequest>::decode(json),
            Err(ProtocolError::UnsupportedVersion { got: 2, .. })
        ));
    }

    #[test]
    fn decode_rejects_unknown_request_type() {
        let json = r#"{"version": 1, "payload": {"type": "FORMAT_DISK"}}"#;
        assert!(matches!(
            Envelope::<ClientRequest>::decode(json),
            Err(ProtocolError::Json(_))
        ));
    }

    #[test]
    fn decode_rejects_malformed_json() {
        assert!(matches!(
            Envelope::<ClientRequest>::decode("{not json"),
            Err(ProtocolError::Json(_))
        ));
    }

    #[test]
    fn resize_to_zero_is_invalid() {
        let req = ClientRequest::Resize {
            session_id: SessionId(1),
            cols: 0,
            rows: 24,
        };
        assert_eq!(
            req.validate(),
            Err(ProtocolError::InvalidTerminalSize { cols: 0, rows: 24 })
        );
    }

    #[test]
    fn create_session_with_zero_rows_is_invalid() {
        let req = ClientRequest::CreateSession {
            shell: None,
            cwd: None,
            cols: 80,
            rows: 0,
            origin: SessionOrigin::Ui,
            title: None,
            command: None,
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn smallest_terminal_is_valid() {
        let req = ClientRequest::Resize {
            session_id: SessionId(1),
            cols: 1,
            rows: 1,
        };
        assert_eq!(req.validate(), Ok(()));
    }

    #[test]
    fn continuation_requires_a_nonzero_session_id() {
        let request = ClientRequest::ContinueSession {
            session_id: SessionId(0),
        };
        assert_eq!(
            request.validate(),
            Err(ProtocolError::InvalidIdentifier {
                field: "session_id"
            })
        );
    }

    #[test]
    fn event_queries_require_a_bounded_limit_and_valid_session() {
        let valid = ClientRequest::GetEvents {
            session_id: Some(SessionId(1)),
            after: Some(crate::EventId(0)),
            limit: 500,
        };
        assert_eq!(valid.validate(), Ok(()));

        let zero = ClientRequest::GetEvents {
            session_id: None,
            after: None,
            limit: 0,
        };
        assert!(matches!(
            zero.validate(),
            Err(ProtocolError::InvalidQueryLimit { actual: 0, .. })
        ));

        let bad_session = ClientRequest::GetEvents {
            session_id: Some(SessionId(0)),
            after: None,
            limit: 1,
        };
        assert!(matches!(
            bad_session.validate(),
            Err(ProtocolError::InvalidIdentifier {
                field: "session_id"
            })
        ));
    }

    #[test]
    fn legacy_create_session_receives_ui_defaults() {
        let json = r#"{"version":1,"payload":{"type":"CREATE_SESSION","shell":null,"cwd":"/tmp","cols":80,"rows":24}}"#;
        let envelope = Envelope::<ClientRequest>::decode(json).unwrap();
        assert!(matches!(
            envelope.payload,
            ClientRequest::CreateSession {
                origin: SessionOrigin::Ui,
                title: None,
                command: None,
                ..
            }
        ));
    }

    #[test]
    fn create_session_validates_origin_cwd_and_structured_command() {
        let request = ClientRequest::CreateSession {
            shell: None,
            cwd: Some(String::new()),
            cols: 80,
            rows: 24,
            origin: SessionOrigin::Watch,
            title: None,
            command: Some(vec!["python".into(), "train.py".into()]),
        };
        assert!(request.validate().unwrap_err().to_string().contains("cwd"));

        let empty = ClientRequest::CreateSession {
            shell: None,
            cwd: None,
            cols: 80,
            rows: 24,
            origin: SessionOrigin::Watch,
            title: None,
            command: Some(Vec::new()),
        };
        assert!(empty.validate().unwrap_err().to_string().contains("argv"));

        let executable = ClientRequest::CreateSession {
            shell: None,
            cwd: None,
            cols: 80,
            rows: 24,
            origin: SessionOrigin::Watch,
            title: None,
            command: Some(vec![String::new()]),
        };
        assert!(executable
            .validate()
            .unwrap_err()
            .to_string()
            .contains("executable"));
    }

    #[test]
    fn local_requests_validate_pid_sequence_and_bounded_text() {
        let bad_pid = LocalRequest::RegisterShell {
            pid: 0,
            shell: "/bin/zsh".into(),
            cwd: "/tmp".into(),
            title: None,
        };
        assert!(bad_pid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("pid 0"));

        let bad_sequence = LocalRequest::ShellCommandStarted {
            session_id: SessionId(1),
            pid: 10,
            sequence: 0,
            text: "true".into(),
            cwd: "/tmp".into(),
            started_at: crate::Timestamp(1),
        };
        assert!(bad_sequence
            .validate()
            .unwrap_err()
            .to_string()
            .contains("sequence 0"));

        let oversized = LocalRequest::ShellCommandStarted {
            session_id: SessionId(1),
            pid: 10,
            sequence: 1,
            text: "x".repeat(MAX_COMMAND_TEXT_BYTES + 1),
            cwd: "/tmp".into(),
            started_at: crate::Timestamp(1),
        };
        assert!(oversized
            .validate()
            .unwrap_err()
            .to_string()
            .contains("command text"));
    }
}
