//! Single-user password authentication HTTP types.

use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// Whether the daemon still needs its owner's password configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthStatus {
    /// True until the owner bootstraps the first password.
    pub setup_required: bool,
}

/// Request to configure the single owner's first password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthSetupRequest {
    /// New owner password.
    pub password: String,
}

/// Request to exchange the owner's password for a browser session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthLoginRequest {
    /// Owner password.
    pub password: String,
}

/// Request to replace the owner's password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthChangePasswordRequest {
    /// Existing owner password.
    pub current_password: String,
    /// Replacement owner password.
    pub new_password: String,
}

/// Browser session issued after setup, login, or password change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthSession {
    /// Random bearer token for the browser session.
    pub token: String,
    /// Absolute session expiry in Unix milliseconds.
    pub expires_at: Timestamp,
}

/// Metadata for a currently authenticated browser session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthSessionInfo {
    /// Absolute session expiry in Unix milliseconds.
    pub expires_at: Timestamp,
}

/// Stable reason for an authentication request failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthFailureCode {
    /// The bootstrap machine token was missing or wrong.
    InvalidBootstrap,
    /// The supplied password was wrong.
    InvalidCredentials,
    /// Too many password attempts were made from one client address.
    Locked,
    /// A password must be configured before login.
    SetupRequired,
    /// A password was already configured.
    AlreadyConfigured,
    /// A new password did not meet the daemon's rules.
    InvalidPassword,
    /// The browser session token was missing, invalid, or expired.
    InvalidSession,
    /// The daemon could not complete authentication safely.
    Internal,
}

/// Structured authentication error returned by the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthFailure {
    /// Machine-readable failure reason.
    pub code: AuthFailureCode,
    /// Human-readable failure detail.
    pub message: String,
    /// Password attempts left before lockout, when applicable.
    pub attempts_remaining: Option<u32>,
    /// Lockout end in Unix milliseconds, when applicable.
    pub retry_at: Option<Timestamp>,
}
