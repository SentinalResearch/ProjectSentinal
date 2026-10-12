//! Control discovery (4.E §5). `GET /api/v1/discovery` is unauthenticated and
//! reachable only through Tailscale Serve; it names the control and nothing
//! else (no owner, Machines, Sessions, or credentials).

use crate::{
    link::validate_display_name, release::strict_version, MachineId, ProtocolError,
    PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};

/// Upper bound on a discovery body; clients refuse anything larger.
pub const MAX_DISCOVERY_BYTES: usize = 1024;
/// The only discovery schema this build understands.
pub const DISCOVERY_SCHEMA_VERSION: u32 = 1;

/// Inclusive range of link protocol versions a control speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProtocolRange {
    /// Oldest supported version.
    pub min: u32,
    /// Newest supported version.
    pub max: u32,
}

/// Pairing methods the control accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingSupport {
    /// Click-to-approve requests with verification words.
    pub approval: bool,
    /// One-time codes (4.0).
    pub code: bool,
}

/// What a control says about itself to a Tailnet peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DiscoveryResponse {
    /// Always `"control"`.
    pub sentinal: String,
    /// [`DISCOVERY_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The control's immutable Machine ID; workers pin it.
    pub control_machine_id: MachineId,
    /// Owner-chosen name, e.g. "Owner's Sentinal".
    pub display_name: String,
    /// The control's public origin; must equal the probed origin.
    pub origin: String,
    /// Installed release.
    pub release: String,
    /// Link protocol versions.
    pub protocol: ProtocolRange,
    /// Pairing methods.
    pub pairing: PairingSupport,
}

impl DiscoveryResponse {
    /// Accept only a compatible control answering for exactly `probed_origin`.
    pub fn validate(&self, probed_origin: &str) -> Result<(), ProtocolError> {
        let fail = |reason| Err(ProtocolError::InvalidDiscovery(reason));
        if self.sentinal != "control" {
            return fail("not a Sentinal control");
        }
        if self.schema_version != DISCOVERY_SCHEMA_VERSION {
            return fail("unsupported discovery schema");
        }
        if self.origin != probed_origin {
            return fail("origin does not match the probed address");
        }
        if !(self.protocol.min..=self.protocol.max).contains(&PROTOCOL_VERSION) {
            return fail("incompatible protocol version");
        }
        if !self.control_machine_id.is_ulid() {
            return fail("control Machine ID is not a ULID");
        }
        if !strict_version(&self.release) {
            return fail("release is not MAJOR.MINOR.PATCH");
        }
        match validate_display_name(&self.display_name) {
            Ok(name) if name == self.display_name => Ok(()),
            _ => fail("display name is not a safe name"),
        }
    }
}
