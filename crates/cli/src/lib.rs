//! Reusable command dispatch, parsing, and terminal framing for `sentinal`.

pub mod adopt_command;
pub mod agent_chat_command;
pub mod agent_state_command;
pub mod app;
pub mod approval_command;
pub mod args;
pub mod auto_command;
pub mod auto_wrap;
pub mod backup;
pub mod client;
pub mod complete_command;
pub mod connect_command;
pub mod daemon_url;
pub mod detach;
pub mod discovery;
pub mod doctor;
mod doctor_pilot;
pub mod doctor_report;
mod doctor_storage;
pub mod frame;
pub mod hook_command;
pub mod hook_report;
pub mod hook_script;
pub mod login;
pub mod mcp;
pub mod pickup;
pub mod rc_file;
pub mod rc_install;
pub mod release;
pub mod release_args;
pub mod session_env;
mod session_size;
pub mod setup;
pub mod skill_install;
pub mod top;
pub mod update;
mod update_guard;
pub mod version;
pub mod watch;
pub mod watch_pickup;

/// The guide printed by `sentinal agent-guide`.
pub const AGENT_GUIDE: &str = include_str!("../../../docs/AGENT_GUIDE.md");

/// The skill file printed by `sentinal agent-skill` (6.4).
pub const AGENT_SKILL: &str = include_str!("../../../docs/AGENT_SKILL.md");
