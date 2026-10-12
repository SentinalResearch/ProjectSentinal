//! Pinned owner release-signing keys, compiled into `sentinald`.

use std::{fs, path::Path};

/// The repository's `packaging/release-keys/allowed_signers`, verbatim.
pub const PINNED_RELEASE_KEYS: &str =
    include_str!("../../../../packaging/release-keys/allowed_signers");

/// Maximum size of an owner-configured extra release-keys file.
pub(super) const MAX_EXTRA_KEYS_BYTES: u64 = 8 * 1024;

/// True for one well-formed `allowed_signers` line naming a release key.
pub(super) fn is_key_line(line: &str) -> bool {
    let mut fields = line.split_whitespace();
    let (Some(identity), Some(namespaces), Some(kind), Some(key)) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return false;
    };
    identity == "sentinal-release"
        && namespaces == "namespaces=\"sentinal-release\""
        && matches!(kind, "ssh-ed25519" | "sk-ssh-ed25519@openssh.com")
        && !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

/// Only the key lines of `text` (comments and blanks dropped).
pub(super) fn key_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| is_key_line(line))
        .collect()
}

/// Reject any line that is neither blank, a comment, nor a key line.
pub(super) fn validate_extra(text: &str) -> Result<(), String> {
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if !(line.is_empty() || line.starts_with('#') || is_key_line(line)) {
            return Err(format!(
                "line {} is not a sentinal-release key line",
                index + 1
            ));
        }
    }
    Ok(())
}

/// The compiled-in pin plus an optional owner-only extra keys file, as key lines.
pub(super) fn load(
    extra: Option<&Path>,
    owner_uid: u32,
) -> Result<String, super::PackageServiceError> {
    let mut lines: Vec<String> = key_lines(PINNED_RELEASE_KEYS)
        .into_iter()
        .map(str::to_owned)
        .collect();
    if let Some(path) = extra {
        let invalid = |reason: String| super::PackageServiceError::ReleaseKeys {
            path: path.to_path_buf(),
            reason,
        };
        let metadata = fs::symlink_metadata(path).map_err(|error| invalid(error.to_string()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid("expected a regular file, not a symlink".into()));
        }
        if !super::is_owned_by(&metadata, owner_uid) || is_shared_writable(&metadata) {
            return Err(invalid(
                "must be owned by this user and not group- or world-writable".into(),
            ));
        }
        if metadata.len() > MAX_EXTRA_KEYS_BYTES {
            return Err(invalid(format!("exceeds {} bytes", MAX_EXTRA_KEYS_BYTES)));
        }
        let text = fs::read_to_string(path).map_err(|error| invalid(error.to_string()))?;
        validate_extra(&text).map_err(invalid)?;
        lines.extend(key_lines(&text).into_iter().map(str::to_owned));
    }
    Ok(lines.into_iter().map(|line| line + "\n").collect())
}

#[cfg(unix)]
fn is_shared_writable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o022 != 0
}

#[cfg(not(unix))]
fn is_shared_writable(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{is_key_line, key_lines, validate_extra};

    const KEY: &str = "sentinal-release namespaces=\"sentinal-release\" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBp1 primary";

    #[test]
    fn recognises_only_sentinal_release_ed25519_lines() {
        assert!(is_key_line(KEY));
        assert!(is_key_line(
            &KEY.replace("ssh-ed25519", "sk-ssh-ed25519@openssh.com")
        ));
        assert!(!is_key_line(&KEY.replace("ssh-ed25519", "ssh-rsa")));
        assert!(!is_key_line(&KEY.replace("sentinal-release ", "someone ")));
        assert!(!is_key_line(&KEY.replace("AAAAC3", "AA$(rm)")));
        assert_eq!(key_lines(&format!("# comment\n\n{KEY}\n")), vec![KEY]);
    }

    #[test]
    fn extra_keys_allow_only_comments_blanks_and_key_lines() {
        assert!(validate_extra(&format!("# dev\n\n{KEY}\n")).is_ok());
        assert!(validate_extra("ssh-ed25519 AAAA stray\n").is_err());
    }
}
