//! Single-user authentication HTTP routes.

use super::AppState;
use crate::auth::AuthError;
use axum::{
    extract::{connect_info::ConnectInfo, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use protocol::{
    AuthChangePasswordRequest, AuthFailure, AuthFailureCode, AuthLoginRequest, AuthSetupRequest,
    AuthStatus,
};
use std::net::SocketAddr;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/status", get(auth_status))
        .route("/api/v1/auth/setup", post(auth_setup))
        .route("/api/v1/auth/login", post(auth_login))
        .route("/api/v1/auth/session", get(auth_session))
        .route("/api/v1/auth/logout", post(auth_logout))
        .route("/api/v1/auth/password", post(auth_change_password))
}

async fn auth_status(State(state): State<AppState>) -> Response {
    match state.auth.setup_required() {
        Ok(setup_required) => Json(AuthStatus { setup_required }).into_response(),
        Err(error) => failure_response(error),
    }
}

async fn auth_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AuthSetupRequest>,
) -> Response {
    let Some(token) = bearer_from_headers(&headers) else {
        return failure_response(AuthError::InvalidBootstrap);
    };
    match state.auth.setup(token, &request.password).await {
        Ok(session) => Json(session).into_response(),
        Err(error) => failure_response(error),
    }
}

async fn auth_login(
    State(state): State<AppState>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    Json(request): Json<AuthLoginRequest>,
) -> Response {
    match state
        .auth
        .login(&address.ip().to_string(), &request.password)
        .await
    {
        Ok(session) => Json(session).into_response(),
        Err(error) => failure_response(error),
    }
}

async fn auth_session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = bearer_from_headers(&headers) else {
        return failure_response(AuthError::InvalidSession);
    };
    match state.auth.session_info(token) {
        Ok(info) => Json(info).into_response(),
        Err(error) => failure_response(error),
    }
}

async fn auth_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = bearer_from_headers(&headers) else {
        return failure_response(AuthError::InvalidSession);
    };
    match state.auth.logout(token) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failure_response(error),
    }
}

async fn auth_change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AuthChangePasswordRequest>,
) -> Response {
    let Some(token) = bearer_from_headers(&headers) else {
        return failure_response(AuthError::InvalidSession);
    };
    match state
        .auth
        .change_password(token, &request.current_password, &request.new_password)
        .await
    {
        Ok(session) => Json(session).into_response(),
        Err(error) => failure_response(error),
    }
}

pub(super) fn bearer_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

fn failure_response(error: AuthError) -> Response {
    let (status, code, attempts_remaining, retry_at, message) = match error {
        AuthError::InvalidBootstrap => (
            StatusCode::UNAUTHORIZED,
            AuthFailureCode::InvalidBootstrap,
            None,
            None,
            "invalid bootstrap machine token".into(),
        ),
        AuthError::InvalidCredentials { attempts_remaining } => (
            StatusCode::UNAUTHORIZED,
            AuthFailureCode::InvalidCredentials,
            Some(attempts_remaining),
            None,
            "invalid owner password".into(),
        ),
        AuthError::Locked { retry_at } => (
            StatusCode::LOCKED,
            AuthFailureCode::Locked,
            Some(0),
            Some(retry_at),
            format!("password login is locked until {}", retry_at.0),
        ),
        AuthError::SetupRequired => (
            StatusCode::CONFLICT,
            AuthFailureCode::SetupRequired,
            None,
            None,
            "owner password setup is required".into(),
        ),
        AuthError::AlreadyConfigured => (
            StatusCode::CONFLICT,
            AuthFailureCode::AlreadyConfigured,
            None,
            None,
            "owner password is already configured".into(),
        ),
        AuthError::InvalidPassword { reason } => (
            StatusCode::BAD_REQUEST,
            AuthFailureCode::InvalidPassword,
            None,
            None,
            reason,
        ),
        AuthError::InvalidSession => (
            StatusCode::UNAUTHORIZED,
            AuthFailureCode::InvalidSession,
            None,
            None,
            "invalid or expired browser session".into(),
        ),
        internal => {
            tracing::error!(error = %internal, "authentication request failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                AuthFailureCode::Internal,
                None,
                None,
                "authentication could not be completed".into(),
            )
        }
    };
    (
        status,
        Json(AuthFailure {
            code,
            message,
            attempts_remaining,
            retry_at,
        }),
    )
        .into_response()
}
