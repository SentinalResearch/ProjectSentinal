//! Privacy-minimized Machine state projected into the control dashboard.

use crate::link::RegistryStatus;
use crate::{
    CaptureLevel, Event, EventKind, Health, HealthLevel, HealthOverall, MachineCapability,
    MachineId, Os, ProtocolError, Resource, Session, SessionId, SessionOrigin, SessionState,
    Timestamp,
};
use serde::{Deserialize, Serialize};

/// Current projection schema understood by this build.
pub const PROJECTION_SCHEMA_VERSION: u16 = 1;
/// Maximum changes in one worker upload.
pub const MAX_PROJECTION_CHANGES: usize = 128;
/// Maximum encoded bytes in one worker upload.
pub const MAX_PROJECTION_BATCH_BYTES: usize = 256 * 1024;
/// Maximum Unicode scalar values retained from a user-authored Session title.
pub const MAX_PROJECTION_TITLE_CHARS: usize = 48;
/// Maximum UTF-8 bytes retained from a user-authored Session title.
pub const MAX_PROJECTION_TITLE_BYTES: usize = 192;

/// Worker's current projection cursor, advertised during a heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProjectionSyncReport {
    /// Projection schema produced by the worker.
    pub schema_version: u16,
    /// Epoch that changes whenever the worker projection identity is reset.
    pub source_epoch: String,
    /// Latest cursor committed to the worker outbox.
    pub current_cursor: u64,
    /// Latest cursor acknowledged durably by the control.
    pub last_acked_cursor: u64,
}

/// Next projection action selected by the control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectionSyncAction {
    /// Upload changes after the committed cursor.
    Upload,
    /// No projection work is currently needed.
    Wait,
    /// Send a complete bounded projection snapshot.
    Snapshot,
    /// The control has projection sync disabled.
    Disabled,
}

/// Projection instructions returned with a heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProjectionSyncDirective {
    /// Whether this control accepts projection uploads.
    pub enabled: bool,
    /// Projection schema accepted by the control.
    pub schema_version: u16,
    /// Latest source cursor durably committed by the control.
    pub committed_cursor: u64,
    /// Maximum encoded upload body.
    pub max_batch_bytes: u32,
    /// Maximum changes in one upload.
    pub max_changes: u16,
    /// Work the worker should perform next.
    pub action: ProjectionSyncAction,
}

/// Display-safe capabilities derived from a Session capture level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProjectedCaptureCapabilities {
    /// A live authoritative route may attach to the screen.
    pub can_attach: bool,
    /// A live authoritative route may send input.
    pub can_send_input: bool,
    /// A live authoritative route may resize the PTY.
    pub can_resize: bool,
    /// A live authoritative route may kill the daemon-owned PTY.
    pub can_kill_daemon_pty: bool,
    /// Structured Commands are available from the worker.
    pub exposes_commands: bool,
    /// Terminal output is available from the worker.
    pub exposes_terminal_output: bool,
    /// Adopted log output is available from the worker.
    pub exposes_log_output: bool,
    /// Process Metrics are available from the worker.
    pub exposes_metrics: bool,
}

impl From<CaptureLevel> for ProjectedCaptureCapabilities {
    fn from(capture: CaptureLevel) -> Self {
        let capabilities = capture.capabilities();
        Self {
            can_attach: capabilities.can_attach,
            can_send_input: capabilities.can_send_input,
            can_resize: capabilities.can_resize,
            can_kill_daemon_pty: capabilities.can_kill_daemon_pty,
            exposes_commands: capabilities.exposes_commands,
            exposes_terminal_output: capabilities.exposes_terminal_output,
            exposes_log_output: capabilities.exposes_log_output,
            exposes_metrics: capabilities.exposes_metrics,
        }
    }
}

/// Deterministic Health without free-form evidence strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProjectedHealth {
    /// Current progress percentage.
    pub progress: Option<f32>,
    /// Activity rating.
    pub activity: HealthLevel,
    /// Resource rating.
    pub resources: HealthLevel,
    /// Stability rating.
    pub stability: HealthLevel,
    /// Count of error observations.
    pub errors: u32,
    /// Overall deterministic rating.
    pub overall: HealthOverall,
}

impl From<&Health> for ProjectedHealth {
    fn from(health: &Health) -> Self {
        Self {
            progress: health.progress,
            activity: health.activity,
            resources: health.resources,
            stability: health.stability,
            errors: health.errors,
            overall: health.overall,
        }
    }
}

/// Bounded Session summary used only for dashboard presentation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProjectedSession {
    /// Session ID on its source Machine.
    pub id: SessionId,
    /// Direct predecessor on the same Machine.
    pub continued_from: Option<SessionId>,
    /// Bounded owner-authored title, when safe.
    pub title: Option<String>,
    /// Last authoritative lifecycle state observed.
    pub state: SessionState,
    /// How the Session entered Sentinal.
    pub origin: SessionOrigin,
    /// Capture fidelity.
    pub capture: CaptureLevel,
    /// Display capabilities derived from capture.
    pub capabilities: ProjectedCaptureCapabilities,
    /// Latest safe deterministic Health.
    pub health: Option<ProjectedHealth>,
    /// Session creation time.
    pub created_at: Timestamp,
    /// Last Session state update time.
    pub updated_at: Timestamp,
    /// Terminal-state timestamp when the Session is finished.
    pub finished_at: Option<Timestamp>,
    /// Known shell exit code.
    pub exit_code: Option<i32>,
}

impl ProjectedSession {
    /// Build a privacy-minimized dashboard summary from an authoritative Session.
    pub fn from_session(session: &Session) -> Self {
        let title = session
            .title
            .as_ref()
            .filter(|title| safe_title(title))
            .cloned();
        Self {
            id: session.id,
            continued_from: session.continued_from,
            title,
            state: session.state,
            origin: session.origin,
            capture: session.capture,
            capabilities: session.capture.into(),
            health: session.health.as_ref().map(ProjectedHealth::from),
            created_at: session.created_at,
            updated_at: session.updated_at,
            finished_at: session.state.is_finished().then_some(session.updated_at),
            exit_code: session.exit_code,
        }
    }
}

fn safe_title(title: &str) -> bool {
    !title.is_empty()
        && title.len() <= MAX_PROJECTION_TITLE_BYTES
        && title.chars().count() <= MAX_PROJECTION_TITLE_CHARS
        && !title.chars().any(char::is_control)
}

/// Normalized progress unit that carries no worker-provided text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectedProgressUnit {
    /// Percentage points.
    Percent,
    /// Training epoch.
    Epoch,
    /// Generic step.
    Step,
    /// Generic item.
    Item,
    /// Unit was not on the allowlist.
    Unknown,
}

impl ProjectedProgressUnit {
    fn from_untrusted(unit: &str) -> Self {
        match unit.trim().to_ascii_lowercase().as_str() {
            "%" | "percent" | "percentage" => Self::Percent,
            "epoch" | "epochs" => Self::Epoch,
            "step" | "steps" => Self::Step,
            "item" | "items" | "test" | "tests" => Self::Item,
            _ => Self::Unknown,
        }
    }
}

/// Safe structured facts for a dashboard Event summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "category", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectedEventFacts {
    /// A Command began; Command text is deliberately omitted.
    CommandStarted,
    /// A Command finished.
    CommandFinished {
        /// Exit code.
        exit_code: i32,
        /// Runtime in milliseconds.
        duration_ms: crate::DurationMs,
    },
    /// A process appeared; name and PID are deliberately omitted.
    ProcessStarted,
    /// A process exited.
    ProcessExited {
        /// Exit code if known.
        exit_code: Option<i32>,
    },
    /// Numeric progress.
    Progress {
        /// Completed units.
        current: u64,
        /// Total units when known.
        total: Option<u64>,
        /// Allowlisted unit.
        unit: ProjectedProgressUnit,
    },
    /// A warning was detected; matched output is omitted.
    Warning,
    /// An error was detected; matched output is omitted.
    Error,
    /// A resource crossed its high threshold.
    ResourceHigh {
        /// Resource category.
        resource: Resource,
        /// Observed percentage.
        percent: f32,
    },
    /// The Session may be waiting for input; prompt text is omitted.
    Waiting,
    /// The Session may be stalled.
    Stalled {
        /// Quiet duration in milliseconds.
        silent_for_ms: crate::DurationMs,
    },
    /// A checkpoint was detected; filesystem path is omitted.
    Checkpoint,
    /// A server started; arbitrary URL is omitted.
    ServerStarted {
        /// Listening port when known.
        port: Option<u16>,
    },
}

/// Bounded Event summary that contains no raw terminal-derived text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProjectedEventSummary {
    /// Source Event ID.
    pub id: crate::EventId,
    /// Source Session ID.
    pub session_id: SessionId,
    /// Event time.
    pub at: Timestamp,
    /// Safe structured facts.
    pub facts: ProjectedEventFacts,
}

impl ProjectedEventSummary {
    /// Remove text and path fields from an authoritative Event.
    pub fn from_event(event: &Event) -> Self {
        let facts = match &event.kind {
            EventKind::CommandStarted { .. } => ProjectedEventFacts::CommandStarted,
            EventKind::CommandFinished {
                exit_code,
                duration_ms,
            } => ProjectedEventFacts::CommandFinished {
                exit_code: *exit_code,
                duration_ms: *duration_ms,
            },
            EventKind::ProcessStarted { .. } => ProjectedEventFacts::ProcessStarted,
            EventKind::ProcessExited { exit_code, .. } => ProjectedEventFacts::ProcessExited {
                exit_code: *exit_code,
            },
            EventKind::ProgressUpdated {
                current,
                total,
                unit,
            } => ProjectedEventFacts::Progress {
                current: *current,
                total: *total,
                unit: ProjectedProgressUnit::from_untrusted(unit),
            },
            EventKind::WarningDetected { .. } => ProjectedEventFacts::Warning,
            EventKind::ErrorDetected { .. } => ProjectedEventFacts::Error,
            EventKind::ResourceHigh { resource, percent } => ProjectedEventFacts::ResourceHigh {
                resource: *resource,
                percent: *percent,
            },
            EventKind::WaitingForInput { .. } => ProjectedEventFacts::Waiting,
            EventKind::PossiblyStalled { silent_for_ms } => ProjectedEventFacts::Stalled {
                silent_for_ms: *silent_for_ms,
            },
            EventKind::CheckpointCreated { .. } => ProjectedEventFacts::Checkpoint,
            EventKind::ServerStarted { port, .. } => {
                ProjectedEventFacts::ServerStarted { port: *port }
            }
        };
        Self {
            id: event.id,
            session_id: event.session_id,
            at: event.at,
            facts,
        }
    }
}

/// Scope of one coalesced Metrics summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectedMetricScope {
    /// Whole Machine.
    Machine,
    /// One Session process tree.
    Session,
}

/// Latest bounded numeric Metrics needed by dashboard cards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProjectedMetrics {
    /// Metrics scope.
    pub scope: ProjectedMetricScope,
    /// Session for Session-scoped Metrics.
    pub session_id: Option<SessionId>,
    /// Observation time.
    pub at: Timestamp,
    /// CPU utilization.
    pub cpu_percent: f32,
    /// RAM bytes used.
    pub ram_used_bytes: u64,
    /// Total RAM bytes when projected.
    pub ram_total_bytes: Option<u64>,
    /// GPU utilization when available.
    pub gpu_percent: Option<f32>,
    /// VRAM bytes used when available.
    pub vram_used_bytes: Option<u64>,
    /// Total VRAM bytes when projected.
    pub vram_total_bytes: Option<u64>,
}

/// One ordered projection mutation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectionChange {
    /// Insert or replace a Session summary.
    SessionUpsert {
        /// Source cursor.
        cursor: u64,
        /// Safe Session summary.
        session: ProjectedSession,
    },
    /// Remove a Session summary.
    SessionTombstone {
        /// Source cursor.
        cursor: u64,
        /// Removed Session ID.
        session_id: SessionId,
        /// Authoritative removal time.
        removed_at: Timestamp,
    },
    /// Append a safe Event summary.
    EventAppend {
        /// Source cursor.
        cursor: u64,
        /// Safe Event summary.
        event: ProjectedEventSummary,
    },
    /// Replace the latest Metrics for a scope.
    MetricsReplace {
        /// Source cursor.
        cursor: u64,
        /// Latest numeric Metrics.
        metrics: ProjectedMetrics,
    },
}

impl ProjectionChange {
    /// Source cursor carried by this change.
    pub fn cursor(&self) -> u64 {
        match self {
            Self::SessionUpsert { cursor, .. }
            | Self::SessionTombstone { cursor, .. }
            | Self::EventAppend { cursor, .. }
            | Self::MetricsReplace { cursor, .. } => *cursor,
        }
    }
}

/// Bounded worker upload, either an ordered delta or one snapshot chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "mode", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectionBatch {
    /// Changes after the control's committed cursor.
    Delta {
        /// Projection schema.
        schema_version: u16,
        /// Claimed Machine, verified against the link identity.
        machine_id: MachineId,
        /// Worker source epoch.
        source_epoch: String,
        /// Cursor already committed by the control.
        base_cursor: u64,
        /// Last cursor in this batch.
        through_cursor: u64,
        /// Worker send time.
        sent_at: Timestamp,
        /// Ordered changes.
        changes: Vec<ProjectionChange>,
    },
    /// One chunk of a complete bounded snapshot.
    Snapshot {
        /// Projection schema.
        schema_version: u16,
        /// Claimed Machine, verified against the link identity.
        machine_id: MachineId,
        /// Worker source epoch.
        source_epoch: String,
        /// Snapshot identity.
        snapshot_id: String,
        /// Source cursor represented by the snapshot.
        snapshot_cursor: u64,
        /// Zero-based chunk number.
        chunk_index: u32,
        /// Whether this is the final chunk.
        final_chunk: bool,
        /// Worker send time.
        sent_at: Timestamp,
        /// Snapshot records.
        records: Vec<ProjectionChange>,
    },
}

impl ProjectionBatch {
    /// Validate schema, identities, bounds, and cursor ordering.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Delta {
                schema_version,
                machine_id,
                source_epoch,
                base_cursor,
                through_cursor,
                changes,
                ..
            } => {
                validate_batch_header(
                    *schema_version,
                    machine_id,
                    source_epoch,
                    changes.len(),
                    false,
                )?;
                if *through_cursor <= *base_cursor {
                    return Err(invalid(
                        "through_cursor",
                        "must be greater than base_cursor",
                    ));
                }
                validate_cursors(changes, *base_cursor, *through_cursor)?;
                if changes.last().map(ProjectionChange::cursor) != Some(*through_cursor) {
                    return Err(invalid(
                        "through_cursor",
                        "must equal the final change cursor",
                    ));
                }
            }
            Self::Snapshot {
                schema_version,
                machine_id,
                source_epoch,
                snapshot_id,
                snapshot_cursor,
                chunk_index,
                final_chunk,
                records,
                ..
            } => {
                let empty_final = records.is_empty() && *chunk_index == 0 && *final_chunk;
                validate_batch_header(
                    *schema_version,
                    machine_id,
                    source_epoch,
                    records.len(),
                    empty_final,
                )?;
                validate_token("snapshot_id", snapshot_id)?;
                let mut previous = 0;
                for record in records {
                    let cursor = record.cursor();
                    if cursor == 0 || cursor < previous || cursor > *snapshot_cursor {
                        return Err(invalid(
                            "records cursor",
                            "must be ordered and within snapshot_cursor",
                        ));
                    }
                    previous = cursor;
                }
            }
        }
        Ok(())
    }
}

fn validate_batch_header(
    schema_version: u16,
    machine_id: &MachineId,
    source_epoch: &str,
    change_count: usize,
    allow_empty: bool,
) -> Result<(), ProtocolError> {
    if schema_version != PROJECTION_SCHEMA_VERSION {
        return Err(invalid("schema_version", "is unsupported"));
    }
    validate_token("machine_id", &machine_id.0)?;
    validate_token("source_epoch", source_epoch)?;
    if (!allow_empty && change_count == 0) || change_count > MAX_PROJECTION_CHANGES {
        return Err(invalid("changes", "must contain between 1 and 128 records"));
    }
    Ok(())
}

fn validate_token(field: &'static str, value: &str) -> Result<(), ProtocolError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(invalid(field, "must be 1-128 printable bytes"));
    }
    Ok(())
}

fn validate_cursors(
    changes: &[ProjectionChange],
    base_cursor: u64,
    through_cursor: u64,
) -> Result<(), ProtocolError> {
    let mut previous = base_cursor;
    for change in changes {
        let cursor = change.cursor();
        if cursor <= previous || cursor > through_cursor {
            return Err(invalid(
                "changes cursor",
                "must be strictly ordered within the batch",
            ));
        }
        previous = cursor;
    }
    Ok(())
}

fn invalid(field: &'static str, reason: &'static str) -> ProtocolError {
    ProtocolError::InvalidProjectionField { field, reason }
}

/// Reason the control requires a worker snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectionResyncReason {
    /// Delta base did not match the committed cursor.
    CursorMismatch,
    /// Worker source epoch changed.
    SourceEpochChanged,
    /// Projection schema is incompatible.
    SchemaMismatch,
    /// Stored projection could not be trusted.
    CorruptProjection,
}

/// Result of durably applying one worker projection upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProjectionAcknowledgement {
    /// Whether the upload was accepted, duplicated, or needs a resync.
    pub status: ProjectionAcknowledgementStatus,
    /// Worker source epoch understood by the control.
    pub source_epoch: String,
    /// Latest cursor durably committed by the control.
    pub committed_cursor: u64,
    /// Resync reason when required.
    pub reason: Option<ProjectionResyncReason>,
}

/// Outcome of a projection upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectionAcknowledgementStatus {
    /// The control committed new changes.
    Accepted,
    /// The same logical upload was already committed.
    Duplicate,
    /// The worker must send a snapshot.
    ResyncRequired,
}

/// Freshness of a control-side Machine projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectionFreshness {
    /// Recently synchronized through a healthy link.
    Live,
    /// Last synchronized state is older than the live threshold.
    Stale,
    /// Registry heartbeat timed out.
    Offline,
    /// Worker explicitly disconnected.
    Disconnected,
    /// Link credential requires refresh.
    AuthExpired,
    /// Owner revoked the Machine.
    Revoked,
}

/// One Machine and its bounded materialized dashboard state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ControlProjectedMachine {
    /// Source Machine ID.
    pub machine_id: MachineId,
    /// Owner-visible Machine name.
    pub display_name: String,
    /// Reported operating system.
    pub os: Option<Os>,
    /// Reported daemon version.
    pub daemon_version: Option<String>,
    /// Reported Machine capabilities.
    pub capabilities: Vec<MachineCapability>,
    /// Registry lifecycle status.
    pub registry_status: RegistryStatus,
    /// Projection freshness independent of Session state.
    pub freshness: ProjectionFreshness,
    /// Last accepted heartbeat.
    pub last_seen: Option<Timestamp>,
    /// Newest worker observation represented.
    pub observed_at: Option<Timestamp>,
    /// Control commit time.
    pub synced_at: Option<Timestamp>,
    /// Latest committed source cursor.
    pub source_cursor: u64,
    /// Bounded Session summaries.
    pub sessions: Vec<ProjectedSession>,
    /// Bounded latest Event summaries.
    pub events: Vec<ProjectedEventSummary>,
    /// Coalesced latest Metrics.
    pub metrics: Vec<ProjectedMetrics>,
    /// Session summaries omitted by response bounds.
    pub omitted_sessions: u32,
}

/// Initial bounded materialized view returned to the owner PWA.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ControlProjectionSnapshot {
    /// Projection schema.
    pub schema_version: u16,
    /// Monotonic control-view revision.
    pub view_revision: u64,
    /// Control generation time.
    pub generated_at: Timestamp,
    /// Bounded Machine projections.
    pub machines: Vec<ControlProjectedMachine>,
    /// Machines omitted by response bounds.
    pub omitted_machines: u32,
    /// Sessions omitted across all returned Machines.
    pub omitted_sessions: u32,
    /// Whether any response bound omitted data.
    pub truncated: bool,
}

/// Reason the browser must fetch a fresh control snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlSnapshotReason {
    /// Client fell behind the retained live-delta window.
    DeltaBoundExceeded,
    /// Client revision does not match the control stream.
    RevisionMismatch,
    /// Projection schema changed.
    SchemaChanged,
}

/// One control-origin live projection message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlProjectionMessage {
    /// Ordered committed projection changes.
    ProjectionDelta {
        /// Machine whose projection changed. Missing only on legacy messages.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        machine_id: Option<MachineId>,
        /// Previous control-view revision.
        from_revision: u64,
        /// New control-view revision.
        to_revision: u64,
        /// Bounded changes.
        changes: Vec<ProjectionChange>,
    },
    /// Client must fetch a new bounded snapshot.
    SnapshotRequired {
        /// Current control-view revision.
        current_revision: u64,
        /// Why incremental delivery cannot continue.
        reason: ControlSnapshotReason,
    },
}
