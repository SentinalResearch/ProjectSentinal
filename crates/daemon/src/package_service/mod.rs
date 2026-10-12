//! Loopback-only Tailnet worker package service.

mod audit;
mod bootstrap;
mod manifest;
mod release_keys;
mod routes;
mod signed;
mod web;

pub use manifest::{ManifestError, ReleaseManifest, ReleaseTarget, TargetArch, TargetOs};
pub use release_keys::PINNED_RELEASE_KEYS;

use crate::config::PackageServiceConfig;
use audit::PackageAudit;
use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use manifest::{valid_version, MAX_MANIFEST_BYTES};
use std::{
    fs,
    path::{Path as FilePath, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

const MAX_DOWNLOADS: usize = 8;
const MAX_REQUESTS_PER_MINUTE: u32 = 120;

/// Render the verified worker bootstrap for one validated release manifest;
/// it trusts only the given `allowed_signers` release keys.
pub fn render_install_script(
    base_url: &str,
    manifest: &ReleaseManifest,
    release_keys: &str,
) -> String {
    bootstrap::generate(base_url, manifest, release_keys)
}

/// Package-service startup and filesystem failures.
#[derive(Debug, thiserror::Error)]
pub enum PackageServiceError {
    /// A package directory or file operation failed.
    #[error("failed to {operation} package path '{}': {source}", path.display())]
    File {
        /// Operation that failed.
        operation: &'static str,
        /// Affected path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// The owner's extra release-keys file is unsafe or malformed.
    #[error("invalid release keys file '{}': {reason}", path.display())]
    ReleaseKeys {
        /// Rejected file.
        path: PathBuf,
        /// Rejection reason.
        reason: String,
    },
    /// A package root was not a safe directory.
    #[error("unsafe package directory '{}': {reason}", path.display())]
    UnsafeDirectory {
        /// Rejected directory.
        path: PathBuf,
        /// Rejection reason.
        reason: String,
    },
}

/// Read-only state for one package listener.
#[derive(Debug, Clone)]
pub struct PackageService {
    root: PathBuf,
    base_url: String,
    max_artifact_bytes: u64,
    audit: PackageAudit,
    downloads: Arc<Semaphore>,
    rate: Arc<Mutex<RateWindow>>,
    owner_uid: u32,
    web_root: Option<PathBuf>,
    release_keys: String,
}

#[derive(Debug)]
struct RateWindow {
    started: Instant,
    requests: u32,
}

impl PackageService {
    /// Open a private package directory and enforce owner-only permissions.
    pub fn open(config: PackageServiceConfig, owner_uid: u32) -> Result<Self, PackageServiceError> {
        prepare_private_directory(&config.directory, owner_uid)?;
        for child in ["channels", "releases", "artifacts", ".staging"] {
            let path = config.directory.join(child);
            prepare_private_directory(&path, owner_uid)?;
        }
        if let Some(web_root) = config.web_root.as_deref() {
            validate_web_directory(web_root, owner_uid)?;
        }
        let release_keys = release_keys::load(config.extra_release_keys.as_deref(), owner_uid)?;
        Ok(Self {
            audit: PackageAudit::new(&config.directory),
            root: config.directory,
            base_url: config.public_base_url,
            max_artifact_bytes: config.max_artifact_bytes,
            downloads: Arc::new(Semaphore::new(MAX_DOWNLOADS)),
            rate: Arc::new(Mutex::new(RateWindow {
                started: Instant::now(),
                requests: 0,
            })),
            owner_uid,
            web_root: config.web_root,
            release_keys,
        })
    }

    /// The release keys this control trusts: the compiled-in pin plus any
    /// owner-configured extra keys, as `allowed_signers` key lines only.
    pub fn release_keys(&self) -> &str {
        &self.release_keys
    }

    fn allow_request(&self) -> bool {
        let Ok(mut rate) = self.rate.lock() else {
            return false;
        };
        if rate.started.elapsed() >= Duration::from_secs(60) {
            rate.started = Instant::now();
            rate.requests = 0;
        }
        if rate.requests >= MAX_REQUESTS_PER_MINUTE {
            return false;
        }
        rate.requests += 1;
        true
    }
}

fn prepare_private_directory(path: &FilePath, owner_uid: u32) -> Result<(), PackageServiceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && is_owned_by(&metadata, owner_uid) => {}
        Ok(metadata) => {
            let reason = if metadata.file_type().is_symlink() {
                "expected a real directory, not a symlink".into()
            } else if !metadata.is_dir() {
                "expected a directory".into()
            } else {
                format!("expected ownership by daemon uid {owner_uid}")
            };
            return Err(PackageServiceError::UnsafeDirectory {
                path: path.to_path_buf(),
                reason,
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|source| PackageServiceError::File {
                operation: "create",
                path: path.to_path_buf(),
                source,
            })?;
            let metadata =
                fs::symlink_metadata(path).map_err(|source| PackageServiceError::File {
                    operation: "inspect newly created",
                    path: path.to_path_buf(),
                    source,
                })?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || !is_owned_by(&metadata, owner_uid)
            {
                return Err(PackageServiceError::UnsafeDirectory {
                    path: path.to_path_buf(),
                    reason: "new directory type or ownership changed during creation".into(),
                });
            }
        }
        Err(source) => {
            return Err(PackageServiceError::File {
                operation: "inspect",
                path: path.to_path_buf(),
                source,
            });
        }
    }
    set_owner_only(path)
}

/// Build the package-only router. It intentionally has no control API routes.
pub fn package_router(service: PackageService) -> Router {
    Router::new()
        .route("/install.sh", get(routes::install_script))
        .route(
            "/packages/v1/channels/stable.json",
            get(routes::stable_manifest),
        )
        .route(
            "/packages/v1/releases/{file}",
            get(routes::release_manifest),
        )
        .route("/packages/v1/artifacts/{file}", get(routes::artifact))
        .fallback(web::serve)
        .with_state(service)
}

fn validate_web_directory(path: &FilePath, owner_uid: u32) -> Result<(), PackageServiceError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| PackageServiceError::File {
        operation: "inspect web root",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || !is_owned_by(&metadata, owner_uid)
    {
        return Err(PackageServiceError::UnsafeDirectory {
            path: path.to_path_buf(),
            reason: format!("expected an owner uid {owner_uid} real directory"),
        });
    }
    let index = path.join("index.html");
    let index_metadata =
        fs::symlink_metadata(&index).map_err(|source| PackageServiceError::File {
            operation: "inspect web entry point",
            path: index.clone(),
            source,
        })?;
    if !index_metadata.is_file()
        || index_metadata.file_type().is_symlink()
        || !is_owned_by(&index_metadata, owner_uid)
    {
        return Err(PackageServiceError::UnsafeDirectory {
            path: index,
            reason: "expected an owner-controlled regular index.html".into(),
        });
    }
    Ok(())
}

async fn read_manifest(
    service: &PackageService,
    path: &FilePath,
) -> Result<(Vec<u8>, ReleaseManifest), Response> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && is_owned_by(&metadata, service.owner_uid) =>
        {
            metadata
        }
        Ok(_) => {
            return Err(audited_error(
                service,
                "MANIFEST",
                None,
                StatusCode::NOT_FOUND,
                "manifest is not a regular file",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(audited_error(
                service,
                "MANIFEST",
                None,
                StatusCode::NOT_FOUND,
                "manifest was not found",
            ));
        }
        Err(_) => {
            return Err(audited_error(
                service,
                "MANIFEST",
                None,
                StatusCode::INTERNAL_SERVER_ERROR,
                "manifest metadata could not be read",
            ));
        }
    };
    if metadata.len() > MAX_MANIFEST_BYTES as u64 {
        return Err(audited_error(
            service,
            "MANIFEST",
            None,
            StatusCode::CONFLICT,
            "release manifest exceeds 65536 bytes",
        ));
    }
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Err(audited_error(
                service,
                "MANIFEST",
                None,
                StatusCode::INTERNAL_SERVER_ERROR,
                "manifest could not be read",
            ));
        }
    };
    let manifest = match ReleaseManifest::parse(&bytes, service.max_artifact_bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(audited_error(
                service,
                "MANIFEST",
                None,
                StatusCode::CONFLICT,
                &error.to_string(),
            ));
        }
    };
    Ok((bytes, manifest))
}

fn artifact_identity(file: &str) -> Option<(&str, &str)> {
    let value = file
        .strip_prefix("sentinal-worker-")?
        .strip_suffix(".tar.gz")?;
    const TARGETS: [&str; 4] = [
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ];
    TARGETS.iter().find_map(|target| {
        let version = value.strip_suffix(&format!("-{target}"))?;
        valid_version(version).then_some((version, *target))
    })
}

fn rate_limit(service: &PackageService) -> Option<Response> {
    (!service.allow_request()).then(|| {
        audited_error(
            service,
            "RATE_LIMIT",
            None,
            StatusCode::TOO_MANY_REQUESTS,
            "package request limit exceeded; retry after one minute",
        )
    })
}

fn audited_error(
    service: &PackageService,
    route: &str,
    release: Option<&str>,
    status: StatusCode,
    message: &str,
) -> Response {
    service
        .audit
        .append(route, release, None, status.as_u16(), 0);
    (status, message.to_string()).into_response()
}

fn response_bytes(status: StatusCode, content_type: &'static str, bytes: Vec<u8>) -> Response {
    let mut response = (status, Body::from(bytes)).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

#[cfg(unix)]
fn set_owner_only(path: &FilePath) -> Result<(), PackageServiceError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        PackageServiceError::File {
            operation: "set owner-only permissions on",
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_owner_only(_path: &FilePath) -> Result<(), PackageServiceError> {
    Ok(())
}

#[cfg(unix)]
fn is_owned_by(metadata: &fs::Metadata, expected_uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == expected_uid
}

#[cfg(not(unix))]
fn is_owned_by(_metadata: &fs::Metadata, _expected_uid: u32) -> bool {
    true
}
