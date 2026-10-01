//! Errors returned by structured persistence and Session lineage operations.

use protocol::{CaptureLevel, SessionId, SessionState};

/// Errors reading or writing structured state.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite rejected an operation.
    #[error("SQLite store operation failed: {0}")]
    Sql(#[from] rusqlite::Error),
    /// The database file could not be created or made owner-only.
    #[error("failed to secure SQLite database '{}': {source}", path.display())]
    File {
        /// Database path.
        path: std::path::PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Persisted JSON could not be encoded or decoded.
    #[error("store JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),
    /// A persisted Session could not make the required lifecycle transition.
    #[error(transparent)]
    Protocol(#[from] protocol::ProtocolError),
    /// Shared connection state was poisoned by a panic.
    #[error("failed to lock SQLite store: lock poisoned")]
    Lock,
    /// An unsigned wire identifier cannot be represented by SQLite.
    #[error("wire identifier {value} is outside SQLite's signed integer range")]
    IdRange {
        /// Identifier that could not be stored.
        value: u64,
    },
    /// An update targeted a record that does not exist.
    #[error("store has no {kind} {id}")]
    Missing {
        /// Record category.
        kind: &'static str,
        /// Missing record identifier.
        id: u64,
    },
    /// A normal Session insert attempted to claim a predecessor.
    #[error("Session {session_id} lineage must be inserted through insert_continuation")]
    LineageInsertRequired {
        /// Session claiming a predecessor.
        session_id: SessionId,
    },
    /// JSON and the authoritative relational lineage column disagree.
    #[error("Session {session_id} lineage mismatch: column is {column:?}, JSON is {json:?}")]
    LineageMismatch {
        /// Session with inconsistent persistence.
        session_id: SessionId,
        /// Relational predecessor.
        column: Option<SessionId>,
        /// JSON predecessor.
        json: Option<SessionId>,
    },
    /// A lineage edge names an absent Session.
    #[error("Session {session_id} lineage references missing Session {missing_id}")]
    MissingLineageSession {
        /// Session whose lineage was being traversed.
        session_id: SessionId,
        /// Referenced Session that is absent.
        missing_id: SessionId,
    },
    /// A Session already has a direct successor.
    #[error("Session {source_id} already has successor Session {successor_id}")]
    SuccessorExists {
        /// Predecessor requested for another continuation.
        source_id: SessionId,
        /// Existing direct successor.
        successor_id: SessionId,
    },
    /// A continuation edge crosses Machine ownership.
    #[error(
        "Session {source_id} on Machine {source_machine} cannot continue as Session {descendant_id} on Machine {descendant_machine}"
    )]
    CrossMachineLineage {
        /// Source Session.
        source_id: SessionId,
        /// Source Machine.
        source_machine: String,
        /// Proposed descendant Session.
        descendant_id: SessionId,
        /// Descendant Machine.
        descendant_machine: String,
    },
    /// A reduced-capture source cannot be continued.
    #[error("Session {session_id} capture level {capture} cannot be continued; FULL required")]
    ContinuationCapture {
        /// Source Session.
        session_id: SessionId,
        /// Rejected capture level.
        capture: CaptureLevel,
    },
    /// A live source cannot be continued.
    #[error("Session {session_id} is in state {state:?} and cannot be continued")]
    ContinuationState {
        /// Source Session.
        session_id: SessionId,
        /// Rejected lifecycle state.
        state: SessionState,
    },
    /// A descendant did not carry the exact requested predecessor.
    #[error(
        "Session {descendant_id} predecessor {actual:?} does not match source Session {source_id}"
    )]
    ContinuationPredecessor {
        /// Source Session.
        source_id: SessionId,
        /// Proposed descendant.
        descendant_id: SessionId,
        /// Predecessor encoded in the descendant.
        actual: Option<SessionId>,
    },
    /// Persisted lineage contains a cycle.
    #[error("Session lineage at {session_id} contains a cycle through Session {cycle_id}")]
    LineageCycle {
        /// Session from which traversal started.
        session_id: SessionId,
        /// Revisited Session.
        cycle_id: SessionId,
    },
    /// Persisted lineage exceeded its corruption guard.
    #[error("Session lineage at {session_id} exceeds {maximum} generations")]
    LineageTooLong {
        /// Session from which traversal started.
        session_id: SessionId,
        /// Maximum traversable generations.
        maximum: usize,
    },
    /// SQLite contained a negative identifier.
    #[error("stored Session identifier {value} is invalid")]
    InvalidStoredId {
        /// Invalid signed database value.
        value: i64,
    },
}
