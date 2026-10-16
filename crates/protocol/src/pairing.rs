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

// ---------------------------------------------------------------------------
// Approval pairing (4.E §6–7)
// ---------------------------------------------------------------------------

use crate::{
    link::{LinkEndpoint, LinkExchangeResponse, LinkMachineInfo, RegistryStatus, Secret},
    Timestamp,
};

/// How long a request stays approvable.
pub const PAIRING_REQUEST_LIFETIME_MS: u64 = 10 * 60 * 1000;
/// Suggested worker poll interval.
pub const PAIRING_POLL_MS: u64 = 2_000;
/// Domain separator for the verification transcript.
pub const VERIFICATION_DOMAIN: &str = "sentinal-pair-v1";

/// Worker → control: ask the owner to approve this Machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingRequestCreate {
    /// Claimed identity; shown to the owner as claimed, never trusted.
    pub machine: LinkMachineInfo,
    /// Where the control calls back for the proof.
    pub endpoint: LinkEndpoint,
    /// SHA-256 (hex) of the worker's 256-bit request secret.
    pub request_secret_sha256: String,
    /// 128-bit worker nonce (hex).
    pub worker_nonce: String,
}

/// Control → worker: the request was recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingRequestCreated {
    /// `pr_` handle.
    pub request_id: String,
    /// The control's Machine ID.
    pub control_machine_id: MachineId,
    /// 128-bit control nonce (hex); words depend on it.
    pub control_nonce: String,
    /// When the request expires.
    pub expires_at: Timestamp,
    /// Suggested claim poll interval.
    pub poll_after_ms: u64,
}

/// Worker → control (callback): proof the worker made this request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingProof {
    /// Request being proven.
    pub request_id: String,
    /// Must equal the request's `request_secret_sha256`.
    pub request_secret_sha256: String,
    /// The control the worker asked.
    pub control_machine_id: MachineId,
}

/// Lifecycle of a pairing request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PairingRequestState {
    /// Waiting for the owner.
    Pending,
    /// Approved; waiting for the worker to claim.
    Approved,
    /// The owner rejected it.
    Rejected,
    /// The worker cancelled it.
    Cancelled,
    /// Ten minutes passed.
    Expired,
    /// The worker claimed its credential.
    Claimed,
}

/// Who Tailscale says the requesting node is (from `tailscale whois`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct TailscaleIdentity {
    /// Node name.
    pub node: String,
    /// Owner login.
    pub login: String,
    /// Node OS as Tailscale reports it.
    pub os: String,
    /// Address the request came from.
    pub address: String,
}

/// A registry Machine the request would reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ExistingMachine {
    /// Current display name.
    pub display_name: String,
    /// Registry status (never ONLINE: an active link is refused).
    pub status: RegistryStatus,
    /// Last heartbeat.
    pub last_seen: Option<Timestamp>,
}

/// One request as the owner sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingRequestView {
    /// `pr_` handle.
    pub request_id: String,
    /// Current state.
    pub state: PairingRequestState,
    /// Expiry.
    pub expires_at: Timestamp,
    /// What the worker claims (untrusted).
    pub claimed: LinkMachineInfo,
    /// Tailscale's identity for the caller; `None` until verified.
    pub tailscale: Option<TailscaleIdentity>,
    /// Whether the callback proof and `whois` both succeeded.
    pub verified: bool,
    /// Present when the claimed ID is a known, inactive Machine.
    pub existing_machine: Option<ExistingMachine>,
    /// The four words the worker also shows.
    pub verification_words: Vec<String>,
}

/// Owner list of requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingRequestList {
    /// Pending and recently decided requests, newest first.
    pub requests: Vec<PairingRequestView>,
}

/// Owner approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingApproval {
    /// Name to record; the claimed name when absent.
    pub display_name: Option<String>,
}

/// Worker → control: claim the approved credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingClaim {
    /// The request secret whose SHA-256 was sent at creation.
    pub request_secret: Secret,
    /// Credential the control will present to this worker.
    pub control_credential: Secret,
}

/// Control → worker: the claim's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingClaimResult {
    /// `PENDING` (poll again), `CLAIMED` (link below), `REJECTED`, `CANCELLED`, or `EXPIRED`.
    pub state: PairingRequestState,
    /// Poll interval while pending.
    pub poll_after_ms: Option<u64>,
    /// The new link, exactly once.
    pub link: Option<LinkExchangeResponse>,
}

/// Worker → control: withdraw a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PairingCancel {
    /// The request secret.
    pub request_secret: Secret,
}

/// Whether `text` is exactly `len` lowercase hex digits.
pub fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether `text` is a `pr_` request handle.
pub fn is_request_id(text: &str) -> bool {
    text.strip_prefix("pr_")
        .is_some_and(|rest| is_hex(rest, 16))
}

impl PairingRequestCreate {
    /// Check shapes only; identity is proven separately.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let fail = |reason| Err(ProtocolError::InvalidPairingRequest(reason));
        if !self.machine.machine_id.is_ulid() {
            return fail("machine ID is not a ULID");
        }
        if validate_display_name(&self.machine.display_name).is_err() {
            return fail("display name is not a safe name");
        }
        if self.machine.daemon_version.len() > 32 || self.machine.capabilities.len() > 16 {
            return fail("machine metadata is too large");
        }
        if !is_hex(&self.request_secret_sha256, 64) || !is_hex(&self.worker_nonce, 32) {
            return fail("secret hash or nonce has the wrong shape");
        }
        Ok(())
    }
}

/// The bytes both sides hash for the verification words: the domain and each
/// field, separated by NUL (none of the fields can contain one).
pub fn verification_transcript(
    control_machine_id: &MachineId,
    request_id: &str,
    control_nonce: &str,
    request_secret_sha256: &str,
    worker_nonce: &str,
) -> Vec<u8> {
    [
        VERIFICATION_DOMAIN,
        &control_machine_id.0,
        request_id,
        control_nonce,
        request_secret_sha256,
        worker_nonce,
    ]
    .join("\0")
    .into_bytes()
}

/// The four words for a SHA-256 digest of the transcript (its first 32 bits).
pub fn verification_words(digest: &[u8]) -> Vec<String> {
    digest
        .iter()
        .take(4)
        .map(|byte| crate::pairing_words::WORDS[usize::from(*byte)].to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Control handoff (a control stepping down moves its workers)
// ---------------------------------------------------------------------------

/// Control → worker (`POST /api/v1/link/move`, control credential): ask the
/// owner of this other control to approve you. The worker keeps its current
/// link until the new one is claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkMoveRequest {
    /// The new control's origin.
    pub control_url: String,
    /// The new control's Machine ID, from discovery.
    pub control_machine_id: MachineId,
}

impl LinkMoveRequest {
    /// Shape checks; the worker verifies the control again when it asks.
    pub fn validate(&self, policy: crate::link::ControlUrlPolicy) -> Result<(), ProtocolError> {
        crate::link::validate_control_url(&self.control_url, policy)?;
        if self.control_machine_id.is_ulid() {
            Ok(())
        } else {
            Err(ProtocolError::InvalidPairingRequest(
                "control Machine ID is not a ULID",
            ))
        }
    }
}

/// What happened to one worker during a handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct WorkerHandoff {
    /// The worker.
    pub machine_id: MachineId,
    /// Its display name.
    pub display_name: String,
    /// It accepted the move and is asking the new control.
    pub moved: bool,
    /// Why not (bounded, no secrets).
    pub message: Option<String>,
}
