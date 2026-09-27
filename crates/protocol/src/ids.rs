//! Typed IDs, so a `SessionId` can never be passed where a `CommandId` is expected.
//!
//! Numeric IDs are unique **per machine** (the daemon hands them out in order).
//! To refer to something across machines, pair it with a [`MachineId`].
//! On the wire every ID is just a plain number or string.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable identifier of a machine, e.g. `"rtx-pc"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(transparent)]
pub struct MachineId(pub String);

/// Identifier of a project, e.g. `"tim"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(transparent)]
pub struct ProjectId(pub String);

/// Identifier of a session, unique on its machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(transparent)]
pub struct SessionId(pub u64);

/// Identifier of a command, unique on its machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(transparent)]
pub struct CommandId(pub u64);

/// Identifier of an event, unique on its machine and increasing over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(transparent)]
pub struct EventId(pub u64);

impl fmt::Display for MachineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl From<&str> for MachineId {
    fn from(s: &str) -> Self {
        MachineId(s.to_string())
    }
}

impl From<&str> for ProjectId {
    fn from(s: &str) -> Self {
        ProjectId(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_ids_serialize_as_plain_numbers() {
        assert_eq!(serde_json::to_string(&SessionId(128)).unwrap(), "128");
        assert_eq!(serde_json::to_string(&CommandId(8192)).unwrap(), "8192");
    }

    #[test]
    fn string_ids_serialize_as_plain_strings() {
        assert_eq!(
            serde_json::to_string(&MachineId::from("rtx-pc")).unwrap(),
            "\"rtx-pc\""
        );
    }

    #[test]
    fn session_id_displays_with_hash() {
        assert_eq!(SessionId(128).to_string(), "#128");
    }
}
