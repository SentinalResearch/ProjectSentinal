//! Machine enrollment: pairing codes, the control registry, and worker links.
//!
//! These are HTTP bodies between an owner browser, the owner-selected control
//! Machine, and worker daemons. Secrets travel as [`Secret`], whose `Debug`
//! and `Display` never print the value.

use crate::{
    MachineId, Os, ProjectionSyncDirective, ProjectionSyncReport, ProtocolError, Timestamp,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Pairing-code symbols: Crockford base32 (no I, L, O, U).
pub const PAIRING_CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Symbols in one pairing code (50 bits).
pub const PAIRING_CODE_LENGTH: usize = 10;
/// Maximum display-name length in Unicode scalar values.
pub const MAX_DISPLAY_NAME_CHARS: usize = 64;

/// A secret string whose `Debug` and `Display` are redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wrap a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// The secret value; call only where it must be sent or compared.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

/// Whether a Machine coordinates others or is coordinated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MachineRole {
    /// Owner-selected registry and coordinator.
    Control,
    /// Enrolled Machine that runs Sessions.
    Worker,
}

/// What a worker daemon can serve to its control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MachineCapability {
    /// Persistent terminal Sessions.
    Sessions,
    /// Watcher Events and Health.
    Events,
    /// Machine and Session Metrics.
    Metrics,
    /// Read-only process adoption.
    Adopt,
}

/// Lifecycle of one pairing code as seen by the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PairingCodeState {
    /// Unused and unexpired.
    Waiting,
    /// Exchanged by a worker.
    Connected,
    /// Its lifetime ended unused.
    Expired,
    /// The owner cancelled it, or a password change or lockout did.
    Cancelled,
}

impl PairingCodeState {
    /// Every state, in declaration order.
    pub const ALL: [PairingCodeState; 4] = [
        PairingCodeState::Waiting,
        PairingCodeState::Connected,
        PairingCodeState::Expired,
        PairingCodeState::Cancelled,
    ];
}

/// A newly created one-time pairing code, returned once to the owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingCode {
    /// Non-secret handle for polling and cancelling.
    pub code_id: String,
    /// The one-time code, formatted `XXXXX-XXXXX`.
    pub code: Secret,
    /// Absolute expiry in Unix milliseconds.
    pub expires_at: Timestamp,
    /// Copyable worker command; never contains the code.
    pub connect_command: String,
}

/// Current state of a pairing code; never contains the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingCodeStatus {
    /// Code handle.
    pub code_id: String,
    /// Current state.
    pub state: PairingCodeState,
    /// Absolute expiry in Unix milliseconds.
    pub expires_at: Timestamp,
    /// Machine that exchanged the code, once connected.
    pub machine_id: Option<MachineId>,
}

/// Tailscale address where the control can reach a worker daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkEndpoint {
    /// Tailscale IPv4 or IPv6 address.
    pub address: String,
    /// Daemon API port.
    pub port: u16,
    /// MagicDNS name, when known.
    pub dns_name: Option<String>,
}

/// Owner-visible registry status of a Machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RegistryStatus {
    /// Heard from within the offline threshold.
    Online,
    /// Linked but silent for longer than the offline threshold.
    Offline,
    /// The worker disconnected itself.
    Disconnected,
    /// The owner revoked the link.
    Revoked,
}

impl RegistryStatus {
    /// Every status, in declaration order.
    pub const ALL: [RegistryStatus; 4] = [
        RegistryStatus::Online,
        RegistryStatus::Offline,
        RegistryStatus::Disconnected,
        RegistryStatus::Revoked,
    ];
}

/// Credential metadata shown to the owner; never contains a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkCredentialInfo {
    /// Link handle.
    pub link_id: String,
    /// When the current credential was issued.
    pub created_at: Timestamp,
    /// When the current credential expires.
    pub expires_at: Timestamp,
    /// Last accepted heartbeat, if any.
    pub last_used_at: Option<Timestamp>,
}

/// One Machine in the control registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RegistryMachine {
    /// Immutable Machine ID.
    pub machine_id: MachineId,
    /// Owner-editable display name; never an authorization identity.
    pub display_name: String,
    /// Control or worker.
    pub role: MachineRole,
    /// Current status.
    pub status: RegistryStatus,
    /// Last accepted heartbeat.
    pub last_seen: Option<Timestamp>,
    /// Where the control reaches this worker.
    pub endpoint: Option<LinkEndpoint>,
    /// Reported operating system.
    pub os: Option<Os>,
    /// Reported daemon version.
    pub daemon_version: Option<String>,
    /// Reported capabilities.
    pub capabilities: Vec<MachineCapability>,
    /// Active credential metadata.
    pub credential: Option<LinkCredentialInfo>,
    /// The latest dashboard-started update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<crate::worker_update::WorkerUpdateStatus>,
}

/// The control registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RegistryMachines {
    /// Every registered Machine, control first.
    pub machines: Vec<RegistryMachine>,
    /// Version on the control's stable channel, when it publishes packages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_release: Option<String>,
}

/// Owner request to rename a Machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RenameMachineRequest {
    /// New display name.
    pub display_name: String,
}

/// What a worker reports about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkMachineInfo {
    /// Immutable Machine ID.
    pub machine_id: MachineId,
    /// Proposed display name.
    pub display_name: String,
    /// Operating system.
    pub os: Os,
    /// Daemon version.
    pub daemon_version: String,
    /// Capabilities.
    pub capabilities: Vec<MachineCapability>,
}

/// Worker → control: exchange a one-time code for a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkExchangeRequest {
    /// The owner-issued one-time code.
    pub pairing_code: Secret,
    /// The worker's identity and metadata.
    pub machine: LinkMachineInfo,
    /// Where the control can reach the worker.
    pub endpoint: LinkEndpoint,
    /// Credential the control will present to this worker (worker-generated).
    pub control_credential: Secret,
}

/// Control → worker: the new link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkExchangeResponse {
    /// Link handle.
    pub link_id: String,
    /// The control's own Machine ID.
    pub control_machine_id: MachineId,
    /// Credential the worker presents to the control.
    pub worker_credential: Secret,
    /// When the credential pair must be refreshed by.
    pub credential_expires_at: Timestamp,
    /// Display name recorded by the control.
    pub display_name: String,
    /// Heartbeat interval.
    pub heartbeat_interval_ms: u64,
}

/// Worker → control liveness and endpoint report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkHeartbeatRequest {
    /// Reporting Machine.
    pub machine_id: MachineId,
    /// Strictly increasing per link.
    pub sequence: u64,
    /// Worker clock at sending.
    pub sent_at: Timestamp,
    /// Current endpoint.
    pub endpoint: LinkEndpoint,
    /// Daemon version.
    pub daemon_version: String,
    /// Capabilities.
    pub capabilities: Vec<MachineCapability>,
    /// Optional projection capability and worker cursor state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_sync: Option<ProjectionSyncReport>,
    /// The latest dashboard-started update, until superseded (4.E §11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_status: Option<crate::worker_update::WorkerUpdateStatus>,
}

/// State of an accepted link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinkStatus {
    /// The link is valid.
    Active,
}

/// Control → worker heartbeat answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkHeartbeatResponse {
    /// Link state.
    pub status: LinkStatus,
    /// Current owner-chosen display name.
    pub display_name: String,
    /// Delay before the next heartbeat.
    pub next_heartbeat_ms: u64,
    /// Whether the worker should refresh its credentials now.
    pub refresh_due: bool,
    /// Whether this worker should forward notification intents to the control.
    ///
    /// Absent when talking to a pre-4.D control and therefore treated as false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_ingress: Option<bool>,
    /// Optional control projection synchronization directive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_sync: Option<ProjectionSyncDirective>,
    /// Version on the control's stable channel, when it publishes packages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_release: Option<String>,
}

/// Worker → control: rotate both credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkRefreshRequest {
    /// Refreshing Machine.
    pub machine_id: MachineId,
    /// New credential the control presents to the worker.
    pub control_credential: Secret,
}

/// Control → worker: the new worker credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkRefreshResponse {
    /// New credential the worker presents.
    pub worker_credential: Secret,
    /// New expiry.
    pub credential_expires_at: Timestamp,
}

/// Worker → control: end this link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkDisconnectRequest {
    /// Disconnecting Machine.
    pub machine_id: MachineId,
}

/// Worker identity proven to its control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkIdentity {
    /// This worker.
    pub machine_id: MachineId,
    /// The control it is linked to.
    pub control_machine_id: MachineId,
}

/// How the control reaches a worker over Tailscale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TailnetPath {
    /// A direct peer-to-peer connection.
    Direct,
    /// Through a Tailscale DERP relay, which adds delay.
    Relayed,
    /// Tailscale did not say (no CLI, no traffic yet, or not a Tailnet address).
    Unknown,
}

/// Result of the control timing its connection to one worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MachinePing {
    /// The worker that was timed.
    pub machine_id: MachineId,
    /// Round trips that succeeded, in microseconds, in the order taken.
    pub round_trips_us: Vec<u32>,
    /// Attempts that failed.
    pub failed: u32,
    /// The last failure's reason; never contains a secret.
    pub error: Option<String>,
    /// How Tailscale reaches the worker from the control.
    pub path: TailnetPath,
    /// The DERP region code when relayed, e.g. `"nyc"`.
    pub relay: Option<String>,
}

/// Stable reason a pairing, registry, or link request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinkFailureCode {
    /// Wrong, expired, used, or cancelled code (indistinguishable on purpose).
    InvalidCode,
    /// Too many wrong codes; pairing is locked for a while.
    PairingLocked,
    /// The worker's identity or metadata is invalid.
    InvalidMachine,
    /// This daemon is not a control Machine.
    NotControl,
    /// The link was revoked by the owner.
    LinkRevoked,
    /// A heartbeat was replayed or out of order.
    StaleSequence,
    /// A browser Origin is not allowed.
    OriginRejected,
    /// The credential was missing or wrong for this route.
    Unauthorized,
    /// The Machine or code is unknown.
    NotFound,
    /// Too many codes were requested.
    RateLimited,
    /// A field failed validation.
    InvalidRequest,
    /// The daemon could not complete the request safely.
    Internal,
    /// An approval request named a Machine that already has an active link.
    MachineAlreadyLinked,
    /// Too many approval requests are pending.
    PairingBusy,
    /// A claim or cancel presented the wrong request secret.
    InvalidClaim,
    /// Live FULL Sessions would be interrupted; the owner must confirm.
    LiveSessions,
}

impl LinkFailureCode {
    /// Every code, in declaration order.
    pub const ALL: [LinkFailureCode; 16] = [
        LinkFailureCode::InvalidCode,
        LinkFailureCode::PairingLocked,
        LinkFailureCode::InvalidMachine,
        LinkFailureCode::NotControl,
        LinkFailureCode::LinkRevoked,
        LinkFailureCode::StaleSequence,
        LinkFailureCode::OriginRejected,
        LinkFailureCode::Unauthorized,
        LinkFailureCode::NotFound,
        LinkFailureCode::RateLimited,
        LinkFailureCode::InvalidRequest,
        LinkFailureCode::Internal,
        LinkFailureCode::MachineAlreadyLinked,
        LinkFailureCode::PairingBusy,
        LinkFailureCode::InvalidClaim,
        LinkFailureCode::LiveSessions,
    ];
}

/// Structured failure body for pairing, registry, and link routes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkFailure {
    /// Machine-readable reason.
    pub code: LinkFailureCode,
    /// Human-readable detail; never contains a secret.
    pub message: String,
}

/// State of this daemon's link to a control, reported to the local CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LocalLinkState {
    /// No link.
    NotConnected,
    /// Linked and heartbeats succeed.
    Active,
    /// Linked, but recent heartbeats failed.
    ControlUnreachable,
    /// The control revoked this Machine.
    Revoked,
}

/// Local view of this daemon's control link; never contains a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ConnectionReport {
    /// Current state.
    pub state: LocalLinkState,
    /// This Machine.
    pub machine_id: MachineId,
    /// Control origin.
    pub control_url: Option<String>,
    /// Control Machine ID.
    pub control_machine_id: Option<MachineId>,
    /// Display name recorded by the control.
    pub display_name: Option<String>,
    /// Link handle.
    pub link_id: Option<String>,
    /// When the link was created.
    pub paired_at: Option<Timestamp>,
    /// Last accepted heartbeat.
    pub last_heartbeat_at: Option<Timestamp>,
    /// Detail for reduced states.
    pub message: Option<String>,
}

/// The copyable worker command for a control origin; it never contains a code.
pub fn connect_command(control_url: &str) -> String {
    format!("sentinal connect {control_url}")
}

/// Trim and validate an owner- or worker-proposed display name.
pub fn validate_display_name(value: &str) -> Result<String, ProtocolError> {
    let trimmed = value.trim();
    let count = trimmed.chars().count();
    if count == 0 {
        return Err(ProtocolError::InvalidDisplayName {
            reason: "must not be empty",
        });
    }
    if count > MAX_DISPLAY_NAME_CHARS {
        return Err(ProtocolError::InvalidDisplayName {
            reason: "must be at most 64 characters",
        });
    }
    let forbidden = |character: char| {
        character.is_control()
            || ('\u{202a}'..='\u{202e}').contains(&character)
            || ('\u{2066}'..='\u{2069}').contains(&character)
    };
    // Check the raw value: some C1 controls (U+0085) count as whitespace.
    if value.chars().any(forbidden) {
        return Err(ProtocolError::InvalidDisplayName {
            reason: "must not contain control or bidirectional-override characters",
        });
    }
    Ok(trimmed.to_string())
}

/// Normalize a typed code: uppercase, drop `-` and spaces, map `O`→`0` and `I`/`L`→`1`.
pub fn normalize_pairing_code(value: &str) -> Result<String, ProtocolError> {
    let mut normalized = String::with_capacity(PAIRING_CODE_LENGTH);
    for character in value.chars() {
        let mapped = match character.to_ascii_uppercase() {
            ' ' | '-' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        if !mapped.is_ascii() || !PAIRING_CODE_ALPHABET.contains(&(mapped as u8)) {
            return Err(ProtocolError::InvalidPairingCode);
        }
        normalized.push(mapped);
    }
    if normalized.len() == PAIRING_CODE_LENGTH {
        Ok(normalized)
    } else {
        Err(ProtocolError::InvalidPairingCode)
    }
}

/// Which control URLs a daemon may connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlUrlPolicy {
    /// Only `https://<name>.<tailnet>.ts.net`.
    Production,
    /// Also `http://` to loopback or a Tailscale IPv4 address, with a port.
    Development,
}

/// Validate and canonicalize a control origin (lowercase, no trailing slash).
pub fn validate_control_url(
    value: &str,
    policy: ControlUrlPolicy,
) -> Result<String, ProtocolError> {
    let invalid = |reason| Err(ProtocolError::InvalidControlUrl { reason });
    let lower = value.trim().to_ascii_lowercase();
    let (scheme, rest) = match lower.split_once("://") {
        Some(parts) => parts,
        None => return invalid("must start with https://"),
    };
    let host_port = rest.strip_suffix('/').unwrap_or(rest);
    if host_port.is_empty()
        || host_port.contains(['/', '?', '#', '@', '\\'])
        || host_port.chars().any(char::is_whitespace)
    {
        return invalid("must be an origin with no path, query, fragment, or credentials");
    }
    match scheme {
        "https" => {
            if host_port.contains(':') {
                return invalid("must not include a port");
            }
            let Some(name) = host_port.strip_suffix(".ts.net") else {
                return invalid("must be a Tailscale .ts.net name");
            };
            let labels: Vec<&str> = name.split('.').collect();
            let valid_label = |label: &&str| {
                !label.is_empty()
                    && label.len() <= 63
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    && !label.starts_with('-')
                    && !label.ends_with('-')
            };
            if labels.len() < 2 || !labels.iter().all(valid_label) {
                return invalid("must be <machine>.<tailnet>.ts.net");
            }
            Ok(format!("https://{host_port}"))
        }
        "http" if policy == ControlUrlPolicy::Development => {
            let Some((host, port)) = host_port.rsplit_once(':') else {
                return invalid("development URLs need an explicit port");
            };
            if port.parse::<u16>().map_or(true, |port| port == 0) {
                return invalid("has an invalid port");
            }
            let Ok(ip) = host.parse::<std::net::Ipv4Addr>() else {
                return invalid("development URLs must use an IPv4 address");
            };
            let tailscale = ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]);
            if !(ip.is_loopback() || tailscale) {
                return invalid("development URLs must use loopback or a Tailscale address");
            }
            Ok(format!("http://{host}:{port}"))
        }
        "http" => invalid("must use https (plain HTTP is development-only)"),
        _ => invalid("must start with https://"),
    }
}

impl MachineId {
    /// Whether this ID is a canonical 26-character uppercase Crockford ULID.
    pub fn is_ulid(&self) -> bool {
        let bytes = self.0.as_bytes();
        bytes.len() == 26
            && bytes[0] <= b'7'
            && bytes
                .iter()
                .all(|byte| PAIRING_CODE_ALPHABET.contains(byte))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphabet_has_no_ambiguous_symbols() {
        for ambiguous in *b"ILOU" {
            assert!(!PAIRING_CODE_ALPHABET.contains(&ambiguous));
        }
    }
}
