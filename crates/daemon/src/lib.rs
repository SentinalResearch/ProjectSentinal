//! Persistent terminal sessions and the daemon API.

pub mod adapter_config;
pub mod api;
pub mod audit;
pub mod auth;
pub mod config;
pub mod entry;
pub mod framing;
pub mod local_socket;
pub mod log_discovery;
pub mod log_tail;
pub mod metrics;
pub mod osc;
pub mod package_service;
pub mod process_probe;
pub mod pty;
pub mod scrollback;
pub mod session_manager;
pub mod shell;
pub mod tracker;
