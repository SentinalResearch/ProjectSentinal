//! Bounded release-manifest parsing and validation.

use protocol::PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Maximum encoded release-manifest length.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
/// Maximum target entries in one release.
pub const MAX_MANIFEST_TARGETS: usize = 16;

/// One immutable worker release manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    /// Manifest schema version, currently one.
    pub schema_version: u32,
    /// Release channel, currently stable.
    pub channel: String,
    /// Strict numeric semantic version.
    pub release_version: String,
    /// Publication time as Unix milliseconds.
    pub published_at: i64,
    /// Oldest compatible terminal protocol.
    pub minimum_protocol_version: u32,
    /// Newest compatible terminal protocol.
    pub maximum_protocol_version: u32,
    /// Exact platform artifacts in this release.
    pub targets: Vec<ReleaseTarget>,
}

/// One platform-specific worker artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseTarget {
    /// Rust-style supported target name.
    pub target: String,
    /// Target operating system.
    pub os: TargetOs,
    /// Target CPU architecture.
    pub arch: TargetArch,
    /// Absolute package-service resource path.
    pub artifact_path: String,
    /// Exact compressed length.
    pub bytes: u64,
    /// Lowercase hexadecimal SHA-256 digest.
    pub sha256: String,
}

/// Supported worker operating systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetOs {
    /// Linux worker.
    Linux,
    /// macOS worker.
    Macos,
}

/// Supported worker CPU architectures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetArch {
    /// 64-bit Intel/AMD.
    X86_64,
    /// 64-bit ARM.
    Aarch64,
}

/// Release-manifest validation failures.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The encoded manifest exceeded its fixed limit.
    #[error("release manifest exceeds {MAX_MANIFEST_BYTES} bytes")]
    TooLarge,
    /// JSON did not match the manifest schema.
    #[error("invalid release manifest JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A field failed validation.
    #[error("invalid release manifest field '{field}': {reason}")]
    Field {
        /// Field name.
        field: &'static str,
        /// Specific failure.
        reason: String,
    },
}

impl ReleaseManifest {
    /// Parse and validate one bounded release manifest.
    pub fn parse(bytes: &[u8], max_artifact_bytes: u64) -> Result<Self, ManifestError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::TooLarge);
        }
        let manifest: Self = serde_json::from_slice(bytes)?;
        manifest.validate(max_artifact_bytes)?;
        Ok(manifest)
    }

    /// Validate this manifest against package-service limits.
    pub fn validate(&self, max_artifact_bytes: u64) -> Result<(), ManifestError> {
        field(self.schema_version == 1, "schema_version", "expected 1")?;
        field(self.channel == "stable", "channel", "expected 'stable'")?;
        field(
            valid_version(&self.release_version),
            "release_version",
            "expected MAJOR.MINOR.PATCH",
        )?;
        field(
            self.published_at >= 0,
            "published_at",
            "must be non-negative",
        )?;
        field(
            self.minimum_protocol_version <= PROTOCOL_VERSION
                && self.maximum_protocol_version >= PROTOCOL_VERSION
                && self.minimum_protocol_version <= self.maximum_protocol_version,
            "protocol",
            &format!("release must include supported protocol {PROTOCOL_VERSION}"),
        )?;
        field(
            !self.targets.is_empty() && self.targets.len() <= MAX_MANIFEST_TARGETS,
            "targets",
            &format!("expected 1..={MAX_MANIFEST_TARGETS} entries"),
        )?;
        let mut targets = HashSet::new();
        for target in &self.targets {
            target.validate(&self.release_version, max_artifact_bytes)?;
            field(
                targets.insert(target.target.as_str()),
                "targets",
                &format!("duplicate target '{}'", target.target),
            )?;
        }
        Ok(())
    }
}

impl ReleaseTarget {
    fn validate(&self, version: &str, max_artifact_bytes: u64) -> Result<(), ManifestError> {
        let expected_target = match (self.os, self.arch) {
            (TargetOs::Linux, TargetArch::X86_64) => "x86_64-unknown-linux-musl",
            (TargetOs::Linux, TargetArch::Aarch64) => "aarch64-unknown-linux-musl",
            (TargetOs::Macos, TargetArch::X86_64) => "x86_64-apple-darwin",
            (TargetOs::Macos, TargetArch::Aarch64) => "aarch64-apple-darwin",
        };
        field(
            self.target == expected_target,
            "target",
            &format!("'{}' does not match OS/architecture", self.target),
        )?;
        let expected_path = format!(
            "/packages/v1/artifacts/sentinal-worker-{version}-{}.tar.gz",
            self.target
        );
        field(
            self.artifact_path == expected_path,
            "artifact_path",
            &format!("expected '{expected_path}'"),
        )?;
        field(
            self.bytes > 0 && self.bytes <= max_artifact_bytes,
            "bytes",
            &format!(
                "artifact length {} exceeds supported range 1..={max_artifact_bytes}",
                self.bytes
            ),
        )?;
        field(
            self.sha256.len() == 64
                && self
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "sha256",
            "expected 64 lowercase hexadecimal SHA-256 characters",
        )
    }
}

/// Return whether a release version is strict numeric MAJOR.MINOR.PATCH.
pub fn valid_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.len() <= 9
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(major), Some(minor), Some(patch), None)
            if valid_part(major) && valid_part(minor) && valid_part(patch)
    )
}

fn field(condition: bool, field: &'static str, reason: &str) -> Result<(), ManifestError> {
    if condition {
        Ok(())
    } else {
        Err(ManifestError::Field {
            field,
            reason: reason.to_string(),
        })
    }
}
