//! Serving each release's owner-signed `SHA256SUMS` and its signature.

use super::{audited_error, is_owned_by, manifest::valid_version, response_bytes, PackageService};
use axum::{http::StatusCode, response::Response};

/// Maximum served `SHA256SUMS` length.
const MAX_SUMS_BYTES: u64 = 64 * 1024;
/// Maximum served signature length.
const MAX_SIGNATURE_BYTES: u64 = 8 * 1024;

/// One of the two signed files stored beside `releases/<version>.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SignedFile {
    /// `<version>.SHA256SUMS`
    Sums,
    /// `<version>.SHA256SUMS.sig`
    Signature,
}

/// Split `<version>.SHA256SUMS[.sig]` into a valid version and file kind.
pub(super) fn parse(file: &str) -> Option<(&str, SignedFile)> {
    let (version, kind) = if let Some(version) = file.strip_suffix(".SHA256SUMS.sig") {
        (version, SignedFile::Signature)
    } else {
        (file.strip_suffix(".SHA256SUMS")?, SignedFile::Sums)
    };
    valid_version(version).then_some((version, kind))
}

/// Serve one signed file verbatim, bounded, owner-only, never a symlink.
pub(super) async fn serve(
    service: &PackageService,
    file: &str,
    version: &str,
    kind: SignedFile,
) -> Response {
    let limit = match kind {
        SignedFile::Sums => MAX_SUMS_BYTES,
        SignedFile::Signature => MAX_SIGNATURE_BYTES,
    };
    let path = service.root.join("releases").join(file);
    let refuse = |status: StatusCode, message: &str| {
        audited_error(service, "SIGNED", Some(version), status, message)
    };
    match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && is_owned_by(&metadata, service.owner_uid) =>
        {
            if metadata.len() == 0 || metadata.len() > limit {
                return refuse(
                    StatusCode::CONFLICT,
                    "signed release file has an invalid size",
                );
            }
        }
        Ok(_) => {
            return refuse(
                StatusCode::NOT_FOUND,
                "signed release file is not a regular file",
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return refuse(
                StatusCode::NOT_FOUND,
                "release is not signed on this control",
            )
        }
        Err(_) => {
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "signed release file could not be read",
            )
        }
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) if bytes.len() as u64 <= limit => {
            service.audit.append(
                "SIGNED",
                Some(version),
                None,
                StatusCode::OK.as_u16(),
                bytes.len() as u64,
            );
            response_bytes(StatusCode::OK, "text/plain; charset=utf-8", bytes)
        }
        _ => refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            "signed release file could not be read",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, SignedFile};

    #[test]
    fn accepts_only_exact_signed_names_for_valid_versions() {
        assert_eq!(
            parse("0.1.18.SHA256SUMS"),
            Some(("0.1.18", SignedFile::Sums))
        );
        assert_eq!(
            parse("0.1.18.SHA256SUMS.sig"),
            Some(("0.1.18", SignedFile::Signature))
        );
        for bad in [
            "0.1.18.json",
            "0.1.18.SHA256SUMS.bak",
            "0.1.18..SHA256SUMS",
            "../0.1.18.SHA256SUMS",
            "x.SHA256SUMS",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
