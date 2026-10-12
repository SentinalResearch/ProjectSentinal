//! Reusable command dispatch, parsing, and terminal framing for `sentinal`.

pub mod adopt_command;
pub mod app;
pub mod args;
pub mod auto_command;
pub mod auto_wrap;
pub mod client;
pub mod connect_command;
pub mod daemon_url;
pub mod detach;
pub mod doctor;
mod doctor_storage;
pub mod frame;
pub mod hook_command;
pub mod hook_report;
pub mod hook_script;
pub mod rc_file;
pub mod rc_install;
pub mod release;
pub mod release_args;
pub mod setup;
pub mod top;
pub mod update;
pub mod version;
pub mod watch;
