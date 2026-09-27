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
//! - [`event`]    — structured things that happened (the system's common language)
//! - [`recipe`]   — reusable command templates
//! - [`process`]  — OS processes and aggregate resource use
//! - [`metric`]   — machine and session resource measurements
//! - [`message`]  — what travels over the WebSocket, wrapped in an [`Envelope`]
//! - [`auth`]     — single-user password authentication over HTTP

pub mod auth;
pub mod command;
pub mod error;
pub mod event;
pub mod ids;
pub mod machine;
pub mod message;
pub mod metric;
pub mod process;
pub mod recipe;
pub mod session;
pub mod time;

pub use auth::{
    AuthChangePasswordRequest, AuthFailure, AuthFailureCode, AuthLoginRequest, AuthSession,
    AuthSessionInfo, AuthSetupRequest, AuthStatus,
};
pub use command::{Command, CommandClass, CommandStatus};
pub use error::ProtocolError;
pub use event::{Event, EventKind, Resource, Severity};
pub use ids::{CommandId, EventId, MachineId, ProjectId, SessionId};
pub use machine::{Gpu, Machine, MachineStatus, Os};
pub use message::{ClientRequest, DaemonMessage, Envelope, PROTOCOL_VERSION};
pub use metric::{GpuMetrics, MachineMetrics, SessionMetrics};
pub use process::{tree_totals, Process, ProcessState, ProcessTotals};
pub use recipe::{Recipe, RecipeParam};
pub use session::{Session, SessionState};
pub use time::{DurationMs, Timestamp};
