//! Authenticated bounded terminal-output replay resource.

use super::AppState;
use crate::session_manager::{
    SessionManagerError, DEFAULT_OUTPUT_PAGE_BYTES, MAX_OUTPUT_PAGE_BYTES,
};
use axum::{
    body::Body,
    extract::{rejection::QueryRejection, Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use protocol::SessionId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(super) struct OutputQuery {
    offset: Option<u64>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct OutputError {
    code: &'static str,
    message: String,
}

pub(super) async fn output(
    State(state): State<AppState>,
    Path(session_id): Path<u64>,
    query: Result<Query<OutputQuery>, QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_OUTPUT_QUERY",
                error.body_text(),
            );
        }
    };
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(DEFAULT_OUTPUT_PAGE_BYTES);
    if limit == 0 || limit > MAX_OUTPUT_PAGE_BYTES {
        return error_response(
            StatusCode::BAD_REQUEST,
            "OUTPUT_RANGE_TOO_LARGE",
            format!("output limit must be between 1 and {MAX_OUTPUT_PAGE_BYTES}, got {limit}"),
        );
    }
    let manager = state.sessions;
    let replay = match tokio::task::spawn_blocking(move || {
        manager.replay_output(SessionId(session_id), offset, limit)
    })
    .await
    {
        Ok(Ok(replay)) => replay,
        Ok(Err(error)) => return manager_error(error),
        Err(error) => {
            tracing::error!(session = session_id, %error, "output replay worker failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "OUTPUT_REPLAY_FAILED",
                format!("saved output for Session {session_id} could not be replayed"),
            );
        }
    };

    let mut response = Response::new(Body::from(replay.bytes));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    insert_header(
        &mut response,
        "x-sentinal-output-offset",
        replay.offset.to_string(),
    );
    insert_header(
        &mut response,
        "x-sentinal-output-next-offset",
        replay.next_offset.to_string(),
    );
    insert_header(
        &mut response,
        "x-sentinal-output-complete",
        replay.complete.to_string(),
    );
    if let Some(warning) = replay.warning {
        response.headers_mut().insert(
            "x-sentinal-output-warning",
            HeaderValue::from_static(warning),
        );
    }
    response
}

fn insert_header(response: &mut Response, name: &'static str, value: String) {
    match HeaderValue::from_str(&value) {
        Ok(value) => {
            response.headers_mut().insert(name, value);
        }
        Err(error) => tracing::error!(%error, header = name, "failed to encode output header"),
    }
}

fn manager_error(error: SessionManagerError) -> Response {
    let (status, code) = match error {
        SessionManagerError::UnknownSession(_) => (StatusCode::NOT_FOUND, "UNKNOWN_SESSION"),
        SessionManagerError::OutputNotFound(_) => (StatusCode::NOT_FOUND, "OUTPUT_NOT_FOUND"),
        SessionManagerError::OutputUnavailable { .. } => {
            (StatusCode::FORBIDDEN, "OUTPUT_UNAVAILABLE")
        }
        SessionManagerError::InvalidOutputRange { .. } => {
            (StatusCode::BAD_REQUEST, "OUTPUT_RANGE_TOO_LARGE")
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "OUTPUT_REPLAY_FAILED"),
    };
    error_response(status, code, error.to_string())
}

fn error_response(status: StatusCode, code: &'static str, message: String) -> Response {
    (status, Json(OutputError { code, message })).into_response()
}
