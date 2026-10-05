//! Single-user authentication HTTP routes.

use super::AppState;
use crate::auth::AuthError;
use axum::{
    extract::{connect_info::ConnectInfo, State},
    http::{
        header::{AUTHORIZATION, COOKIE, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use protocol::{
    AuthChangePasswordRequest, AuthFailure, AuthFailureCode, AuthLoginRequest, AuthSession,
    AuthSessionInfo, AuthSetupRequest, AuthStatus,
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
        .route("/api/v1/auth/browser/setup", post(browser_setup))
        .route("/api/v1/auth/browser/login", post(browser_login))
        .route("/api/v1/auth/browser/session", get(browser_session))
        .route("/api/v1/auth/browser/logout", post(browser_logout))
        .route(
            "/api/v1/auth/browser/password",
            post(browser_change_password),
        )
}

const PRODUCTION_COOKIE: &str = "__Host-sentinal_owner";
const DEVELOPMENT_COOKIE: &str = "sentinal_owner_dev";
const MAX_COOKIE_HEADER_BYTES: usize = 8 * 1024;
const SESSION_MAX_AGE_SECONDS: u64 = 24 * 60 * 60;

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
    headers: HeaderMap,
    Json(request): Json<AuthLoginRequest>,
) -> Response {
    let client = state.client_key(&headers, address);
    match state.auth.login(&client, &request.password).await {
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
        Ok(session) => {
            // Outstanding pairing codes were authorized by the old password.
            if let Some(control) = &state.control {
                if let Err(failure) = control.cancel_all_codes("PASSWORD_CHANGED") {
                    tracing::warn!(
                        ?failure,
                        "failed to cancel pairing codes after a password change"
                    );
                }
            }
            Json(session).into_response()
        }
        Err(error) => failure_response(error),
    }
}

async fn browser_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AuthSetupRequest>,
) -> Response {
    let Some(token) = bearer_from_headers(&headers) else {
        return failure_response(AuthError::InvalidBootstrap);
    };
    match state.auth.setup(token, &request.password).await {
        Ok(session) => browser_session_response(session, state.development),
        Err(error) => failure_response(error),
    }
}

async fn browser_login(
    State(state): State<AppState>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<AuthLoginRequest>,
) -> Response {
    let client = state.client_key(&headers, address);
    match state.auth.login(&client, &request.password).await {
        Ok(session) => browser_session_response(session, state.development),
        Err(error) => failure_response(error),
    }
}

async fn browser_session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = cookie_from_headers(&headers, state.development) else {
        return failure_response(AuthError::InvalidSession);
    };
    match state.auth.session_info(token) {
        Ok(info) => Json(info).into_response(),
        Err(error) => failure_response(error),
    }
}

async fn browser_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = cookie_from_headers(&headers, state.development) else {
        return failure_response(AuthError::InvalidSession);
    };
    match state.auth.logout(token) {
        Ok(()) => clear_cookie_response(state.development),
        Err(error) => failure_response(error),
    }
}

async fn browser_change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AuthChangePasswordRequest>,
) -> Response {
    let Some(token) = cookie_from_headers(&headers, state.development) else {
        return failure_response(AuthError::InvalidSession);
    };
    match state
        .auth
        .change_password(token, &request.current_password, &request.new_password)
        .await
    {
        Ok(session) => {
            cancel_pairing_codes(&state);
            browser_session_response(session, state.development)
        }
        Err(error) => failure_response(error),
    }
}

fn cancel_pairing_codes(state: &AppState) {
    if let Some(control) = &state.control {
        if let Err(failure) = control.cancel_all_codes("PASSWORD_CHANGED") {
            tracing::warn!(
                ?failure,
                "failed to cancel pairing codes after a password change"
            );
        }
    }
}

fn browser_session_response(session: AuthSession, development: bool) -> Response {
    let cookie = format!(
        "{}={}; Path=/; Max-Age={SESSION_MAX_AGE_SECONDS}; HttpOnly; SameSite=Strict{}",
        cookie_name(development),
        session.token,
        if development { "" } else { "; Secure" }
    );
    let mut response = Json(AuthSessionInfo {
        expires_at: session.expires_at,
    })
    .into_response();
    match HeaderValue::from_str(&cookie) {
        Ok(value) => {
            response.headers_mut().insert(SET_COOKIE, value);
            response
        }
        Err(error) => {
            tracing::error!(%error, "failed to create browser session cookie");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn clear_cookie_response(development: bool) -> Response {
    let cookie = format!(
        "{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict{}",
        cookie_name(development),
        if development { "" } else { "; Secure" }
    );
    let mut response = StatusCode::NO_CONTENT.into_response();
    match HeaderValue::from_str(&cookie) {
        Ok(value) => {
            response.headers_mut().insert(SET_COOKIE, value);
            response
        }
        Err(error) => {
            tracing::error!(%error, "failed to clear browser session cookie");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The bearer token in an `Authorization: Bearer` header.
pub fn bearer_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

/// Browser or bearer credential accepted by an owner-authenticated route.
pub fn credential_from_headers(headers: &HeaderMap, development: bool) -> Option<String> {
    bearer_from_headers(headers)
        .map(str::to_string)
        .or_else(|| cookie_from_headers(headers, development).map(str::to_string))
}

fn cookie_from_headers(headers: &HeaderMap, development: bool) -> Option<&str> {
    let value = headers.get(COOKIE)?.to_str().ok()?;
    if value.len() > MAX_COOKIE_HEADER_BYTES {
        return None;
    }
    let expected = cookie_name(development);
    value.split(';').find_map(|part| {
        let (name, token) = part.trim().split_once('=')?;
        (name == expected && !token.is_empty()).then_some(token)
    })
}

fn cookie_name(development: bool) -> &'static str {
    if development {
        DEVELOPMENT_COOKIE
    } else {
        PRODUCTION_COOKIE
    }
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
