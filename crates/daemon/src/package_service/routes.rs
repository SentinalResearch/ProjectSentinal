//! HTTP handlers for the read-only package resources.

use super::{
    artifact_identity, audited_error, is_owned_by, rate_limit, read_manifest,
    render_install_script, response_bytes, PackageService,
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use futures_util::stream;
use std::io;
use tokio::io::AsyncReadExt;

pub(super) async fn install_script(State(service): State<PackageService>) -> Response {
    if let Some(response) = rate_limit(&service) {
        return response;
    }
    let bytes = match read_manifest(&service, &service.root.join("channels/stable.json")).await {
        Ok((_, manifest)) => {
            let script = render_install_script(&service.base_url, &manifest);
            if script.len() > super::manifest::MAX_MANIFEST_BYTES {
                return audited_error(
                    &service,
                    "INSTALL",
                    None,
                    StatusCode::CONFLICT,
                    "generated install script exceeds 65536 bytes",
                );
            }
            script.into_bytes()
        }
        Err(response) => return response,
    };
    service.audit.append(
        "INSTALL",
        None,
        None,
        StatusCode::OK.as_u16(),
        bytes.len() as u64,
    );
    response_bytes(StatusCode::OK, "text/x-shellscript; charset=utf-8", bytes)
}

pub(super) async fn stable_manifest(State(service): State<PackageService>) -> Response {
    if let Some(response) = rate_limit(&service) {
        return response;
    }
    match read_manifest(&service, &service.root.join("channels/stable.json")).await {
        Ok((bytes, manifest)) => {
            service.audit.append(
                "CHANNEL",
                Some(&manifest.release_version),
                None,
                StatusCode::OK.as_u16(),
                bytes.len() as u64,
            );
            response_bytes(StatusCode::OK, "application/json", bytes)
        }
        Err(response) => response,
    }
}

pub(super) async fn release_manifest(
    State(service): State<PackageService>,
    Path(file): Path<String>,
) -> Response {
    if let Some(response) = rate_limit(&service) {
        return response;
    }
    let Some(version) = file
        .strip_suffix(".json")
        .filter(|value| super::manifest::valid_version(value))
    else {
        return audited_error(
            &service,
            "RELEASE",
            None,
            StatusCode::BAD_REQUEST,
            "invalid release manifest name",
        );
    };
    let path = service.root.join("releases").join(&file);
    match read_manifest(&service, &path).await {
        Ok((bytes, manifest)) if manifest.release_version == version => {
            service.audit.append(
                "RELEASE",
                Some(version),
                None,
                StatusCode::OK.as_u16(),
                bytes.len() as u64,
            );
            response_bytes(StatusCode::OK, "application/json", bytes)
        }
        Ok(_) => audited_error(
            &service,
            "RELEASE",
            Some(version),
            StatusCode::CONFLICT,
            "release filename does not match manifest version",
        ),
        Err(response) => response,
    }
}

pub(super) async fn artifact(
    State(service): State<PackageService>,
    Path(file): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = rate_limit(&service) {
        return response;
    }
    if headers.contains_key(header::RANGE) {
        return audited_error(
            &service,
            "ARTIFACT",
            None,
            StatusCode::RANGE_NOT_SATISFIABLE,
            "range requests are not supported; retry the complete verified artifact",
        );
    }
    let Some((version, target_name)) = artifact_identity(&file) else {
        return audited_error(
            &service,
            "ARTIFACT",
            None,
            StatusCode::BAD_REQUEST,
            "invalid artifact name",
        );
    };
    let manifest_path = service
        .root
        .join("releases")
        .join(format!("{version}.json"));
    let (_, manifest) = match read_manifest(&service, &manifest_path).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let resource = format!("/packages/v1/artifacts/{file}");
    let Some(target) = manifest
        .targets
        .iter()
        .find(|target| target.target == target_name && target.artifact_path == resource)
    else {
        return audited_error(
            &service,
            "ARTIFACT",
            Some(version),
            StatusCode::NOT_FOUND,
            "artifact is not declared by the release manifest",
        );
    };
    let path = service.root.join("artifacts").join(&file);
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && is_owned_by(&metadata, service.owner_uid) =>
        {
            metadata
        }
        Ok(_) => {
            return audited_error(
                &service,
                "ARTIFACT",
                Some(version),
                StatusCode::NOT_FOUND,
                "artifact is not a regular file",
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return audited_error(
                &service,
                "ARTIFACT",
                Some(version),
                StatusCode::NOT_FOUND,
                "artifact was not found",
            );
        }
        Err(_) => {
            return audited_error(
                &service,
                "ARTIFACT",
                Some(version),
                StatusCode::INTERNAL_SERVER_ERROR,
                "artifact metadata could not be read",
            );
        }
    };
    if metadata.len() != target.bytes {
        return audited_error(
            &service,
            "ARTIFACT",
            Some(version),
            StatusCode::CONFLICT,
            "artifact length does not match its verified manifest",
        );
    }
    let permit = match service.downloads.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return audited_error(
                &service,
                "ARTIFACT",
                Some(version),
                StatusCode::TOO_MANY_REQUESTS,
                "too many concurrent artifact downloads",
            );
        }
    };
    let file_handle = match tokio::fs::File::open(&path).await {
        Ok(file_handle) => file_handle,
        Err(_) => {
            return audited_error(
                &service,
                "ARTIFACT",
                Some(version),
                StatusCode::INTERNAL_SERVER_ERROR,
                "artifact could not be opened",
            );
        }
    };
    service.audit.append(
        "ARTIFACT",
        Some(version),
        Some(target_name),
        StatusCode::OK.as_u16(),
        target.bytes,
    );
    let body_stream = stream::try_unfold(
        (file_handle, permit),
        |(mut file_handle, permit)| async move {
            let mut buffer = vec![0_u8; 64 * 1024];
            let read = file_handle.read(&mut buffer).await?;
            if read == 0 {
                Ok::<_, io::Error>(None)
            } else {
                buffer.truncate(read);
                Ok(Some((Bytes::from(buffer), (file_handle, permit))))
            }
        },
    );
    let mut response = Response::new(Body::from_stream(body_stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/gzip"),
    );
    if let Ok(value) = HeaderValue::from_str(&target.bytes.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    response
}

pub(super) async fn not_found(State(service): State<PackageService>) -> Response {
    if let Some(response) = rate_limit(&service) {
        return response;
    }
    audited_error(
        &service,
        "UNKNOWN",
        None,
        StatusCode::NOT_FOUND,
        "package resource was not found",
    )
}
