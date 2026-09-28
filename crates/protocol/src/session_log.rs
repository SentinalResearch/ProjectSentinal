//! Log metadata attached to an adopted Session (recorded in 1b.C, tailed in 1b.D).

use serde::{Deserialize, Serialize};

/// Where an adopted process's log path came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LogSource {
    /// Named by the user with `sentinal adopt --log`.
    Explicit,
    /// The process's stdout is redirected to this file.
    Stdout,
    /// The process's stderr is redirected to this file.
    Stderr,
    /// Both stdout and stderr are redirected to this file.
    StdoutAndStderr,
}

/// A regular file that holds an adopted process's output.
///
/// Its presence does not change capture: until the log is tailed, the Session
/// stays `PROCESS` and interfaces say "log detected; tailing not enabled yet".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionLog {
    /// Absolute file path.
    pub path: String,
    /// How the path was found.
    pub source: LogSource,
}
