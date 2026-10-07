//! Bounded production PWA assets on the private package listener.

use super::{is_owned_by, PackageService};
use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use std::path::{Path, PathBuf};

const MAX_WEB_FILE_BYTES: u64 = 8 * 1024 * 1024;
const CSP: &str = "default-src 'self'; connect-src 'self' wss:; font-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'";

pub(super) async fn serve(
    State(service): State<PackageService>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let Some(root) = service.web_root.as_deref() else {
        return super::routes::not_found(State(service)).await;
    };
    let path = uri.path();
    if path.starts_with("/api/") || path.starts_with("/packages/") || path == "/install.sh" {
        return not_found();
    }
    let Some(selected) = selected_file(path) else {
        return not_found();
    };
    read_file(
        root,
        &selected.relative,
        selected.content_type,
        selected.cache,
        service.owner_uid,
    )
    .await
}

struct SelectedFile {
    relative: String,
    content_type: &'static str,
    cache: &'static str,
}

fn selected_file(path: &str) -> Option<SelectedFile> {
    if path == "/manifest.webmanifest" {
        return Some(SelectedFile {
            relative: "manifest.webmanifest".into(),
            content_type: "application/manifest+json",
            cache: "no-cache",
        });
    }
    if matches!(path, "/sw.js" | "/service-worker.js" | "/push-safety.js") {
        return Some(SelectedFile {
            relative: path[1..].into(),
            content_type: "text/javascript; charset=utf-8",
            cache: "no-cache",
        });
    }
    for (prefix, directory, cache) in [
        ("/assets/", "assets", "public, max-age=31536000, immutable"),
        ("/icons/", "icons", "no-cache"),
    ] {
        if let Some(file) = path.strip_prefix(prefix) {
            if safe_filename(file) {
                let content_type = mime(file)?;
                return Some(SelectedFile {
                    relative: format!("{directory}/{file}"),
                    content_type,
                    cache,
                });
            }
            return None;
        }
    }
    if matches!(path, "/" | "/login" | "/setup" | "/app") || path.starts_with("/app/") {
        return Some(SelectedFile {
            relative: "index.html".into(),
            content_type: "text/html; charset=utf-8",
            cache: "no-cache",
        });
    }
    None
}

fn safe_filename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn mime(file: &str) -> Option<&'static str> {
    match Path::new(file).extension().and_then(|value| value.to_str()) {
        Some("js") => Some("text/javascript; charset=utf-8"),
        Some("css") => Some("text/css; charset=utf-8"),
        Some("svg") => Some("image/svg+xml"),
        Some("png") => Some("image/png"),
        Some("woff") => Some("font/woff"),
        Some("woff2") => Some("font/woff2"),
        _ => None,
    }
}

async fn read_file(
    root: &Path,
    relative: &str,
    content_type: &'static str,
    cache: &'static str,
    owner_uid: u32,
) -> Response {
    let path: PathBuf = root.join(relative);
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && is_owned_by(&metadata, owner_uid)
                && metadata.len() <= MAX_WEB_FILE_BYTES =>
        {
            metadata
        }
        _ => return not_found(),
    };
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) if bytes.len() as u64 == metadata.len() => bytes,
        _ => return not_found(),
    };
    secured_response(StatusCode::OK, content_type, cache, Body::from(bytes))
}

fn not_found() -> Response {
    secured_response(
        StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        "no-store",
        Body::from("not found"),
    )
}

fn secured_response(
    status: StatusCode,
    content_type: &'static str,
    cache: &'static str,
    body: Body,
) -> Response {
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("content-security-policy", HeaderValue::from_static(CSP));
    response
}
