//! Messages that travel over the WebSocket between clients and the daemon.
//!
//! Every JSON message is wrapped in an [`Envelope`] with a protocol version.
//! Raw terminal bytes do **not** use these types; they go in binary frames.

use crate::{
    Command, Event, Machine, MachineMetrics, ProtocolError, Session, SessionId, SessionMetrics,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Current protocol version. Bump when a change breaks old clients.
pub const PROTOCOL_VERSION: u32 = 1;

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
    /// Search command text using the daemon's full-text index.
    SearchCommands {
        /// Full-text query entered by the user.
        query: String,
        /// Maximum number of commands to return.
        limit: u32,
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
            _ => Ok(()),
        }
    }
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
}
