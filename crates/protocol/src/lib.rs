//! Shared wire types for terminal-agent.
//!
//! This crate is the contract between the daemon, the web UI, the CLI and the
//! intelligence layer. It contains only data types and small, pure helper
//! functions — no I/O, no async, no clocks.
//!
//! Module map:
//! - [`ids`]      — typed IDs (`SessionId`, `CommandId`, ...)
//! - [`time`]     — `Timestamp` and `DurationMs`
//! - [`machine`]  — a computer running the daemon
//! - [`session`]  — a persistent terminal and its state machine
//! - [`command`]  — one command typed into a session
//! - [`session_log`] — log metadata for adopted processes
//! - [`command_line`] — rendering structured Watch argv as quoted shell input
//! - [`event`]    — structured things that happened (the system's common language)
//! - [`health`]   — deterministic current Session assessment
//! - [`recipe`]   — reusable command templates
//! - [`process`]  — OS processes and aggregate resource use
//! - [`metric`]   — machine and session resource measurements
//! - [`message`]  — what travels over the WebSocket, wrapped in an [`Envelope`]
//! - [`local`]    — private local socket location
//! - [`auth`]     — single-user password authentication over HTTP
//! - [`link`]     — Machine enrollment, the control registry, and worker links

pub mod auth;
pub mod command;
pub mod command_line;
pub mod error;
pub mod event;
pub mod evidence;
pub mod health;
pub mod ids;
pub mod link;
pub mod local;
pub mod machine;
pub mod message;
pub mod metric;
pub mod process;
pub mod recipe;
pub mod session;
pub mod session_log;
pub mod time;

pub use auth::{
    AuthChangePasswordRequest, AuthFailure, AuthFailureCode, AuthLoginRequest, AuthSession,
    AuthSessionInfo, AuthSetupRequest, AuthStatus,
};
pub use command::{Command, CommandClass, CommandStatus};
pub use command_line::{render_command_line, ShellDialect, MAX_RENDERED_COMMAND_BYTES};
pub use error::ProtocolError;
pub use event::{Event, EventKind, Resource, Severity};
pub use health::{
    Health, HealthLevel, HealthOverall, MAX_HEALTH_EVIDENCE, MAX_HEALTH_EVIDENCE_BYTES,
};
pub use ids::{CommandId, EventId, MachineId, ProjectId, SessionId};
pub use link::{MachineCapability, MachineRole, Secret};
pub use machine::{Gpu, Machine, MachineStatus, Os};
pub use message::{
    ClientRequest, DaemonMessage, Envelope, LocalErrorCode, LocalRequest, LocalResponse,
    MAX_COMMAND_TEXT_BYTES, MAX_LOCAL_FRAME_BYTES, PROTOCOL_VERSION,
};
pub use metric::{GpuMetrics, MachineMetrics, SessionMetrics};
pub use process::{tree_totals, Process, ProcessState, ProcessTotals};
pub use recipe::{Recipe, RecipeParam};
pub use session::{CaptureCapabilities, CaptureLevel, Session, SessionOrigin, SessionState};
pub use session_log::{LogSource, SessionLog};
pub use time::{DurationMs, Timestamp};
