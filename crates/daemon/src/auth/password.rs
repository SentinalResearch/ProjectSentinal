//! Persistent owner password and ephemeral browser sessions.

use super::{constant_time_eq, random_hex_token, secure_random, AuthError};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use protocol::{AuthSession, AuthSessionInfo, Timestamp};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const MIN_PASSWORD_BYTES: usize = 12;
const MAX_PASSWORD_BYTES: usize = 1_024;
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_MS: i64 = 5 * 60 * 1_000;
/// Failed logins allowed across *all* client keys per window (F10): behind
/// Tailscale Serve the per-client key is a header a local process can vary.
const GLOBAL_MAX_FAILURES: usize = 20;
const GLOBAL_WINDOW_MS: i64 = 15 * 60 * 1_000;

type Clock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

/// Kind of bearer credential accepted by the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    /// Long-lived machine token used to bootstrap setup and by the local CLI.
    MachineToken,
    /// Expiring token issued to one signed-in browser.
    BrowserSession,
}

#[derive(Debug, Serialize, Deserialize)]
struct PasswordFile {
    password_hash: String,
}

#[derive(Debug, Clone)]
struct BrowserSession {
    token: String,
    expires_at: Timestamp,
}

#[derive(Debug, Default)]
struct LoginFailure {
    attempts: u32,
    locked_until: Option<Timestamp>,
}

#[derive(Default)]
struct AuthState {
    password_hash: Option<String>,
    sessions: Vec<BrowserSession>,
    failures: HashMap<String, LoginFailure>,
    /// Times of recent failed logins from any client, newest last.
    global_failures: Vec<Timestamp>,
    /// Every client is refused until then.
    global_locked_until: Option<Timestamp>,
}

/// Single-owner password state shared by daemon API handlers.
#[derive(Clone)]
pub struct AuthManager {
    password_file: PathBuf,
    machine_token: String,
    session_lifetime_ms: i64,
    clock: Clock,
    state: Arc<Mutex<AuthState>>,
}

impl AuthManager {
    /// Load persistent owner authentication using a 24-hour browser-session lifetime.
    pub fn load(password_file: &Path, machine_token: String) -> Result<Self, AuthError> {
        Self::load_with_clock(
            password_file,
            machine_token,
            24 * 60 * 60 * 1_000,
            Arc::new(system_time),
        )
    }

    /// Load authentication with an injected lifetime and clock for deterministic tests.
    pub fn load_with_clock(
        password_file: &Path,
        machine_token: String,
        session_lifetime_ms: i64,
        clock: Clock,
    ) -> Result<Self, AuthError> {
        let password_hash = load_password_hash(password_file)?;
        Ok(Self {
            password_file: password_file.to_path_buf(),
            machine_token,
            session_lifetime_ms,
            clock,
            state: Arc::new(Mutex::new(AuthState {
                password_hash,
                ..AuthState::default()
            })),
        })
    }

    /// Return true until the owner password has been configured.
    pub fn setup_required(&self) -> Result<bool, AuthError> {
        Ok(self.lock()?.password_hash.is_none())
    }

    /// Configure the first owner password using the machine bootstrap token.
    pub async fn setup(&self, bootstrap: &str, password: &str) -> Result<AuthSession, AuthError> {
        if !constant_time_eq(bootstrap, &self.machine_token) {
            return Err(AuthError::InvalidBootstrap);
        }
        validate_password(password)?;
        if !self.setup_required()? {
            return Err(AuthError::AlreadyConfigured);
        }
        let hash = hash_password(password.to_string()).await?;
        let mut state = self.lock()?;
        if state.password_hash.is_some() {
            return Err(AuthError::AlreadyConfigured);
        }
        persist_password_hash(&self.password_file, &hash)?;
        state.password_hash = Some(hash);
        self.issue_session(&mut state)
    }

    /// Verify the owner password and issue a browser session.
    pub async fn login(&self, client: &str, password: &str) -> Result<AuthSession, AuthError> {
        let now = (self.clock)();
        let hash = {
            let mut state = self.lock()?;
            let hash = state
                .password_hash
                .clone()
                .ok_or(AuthError::SetupRequired)?;
            if let Some(locked_until) = state.global_locked_until {
                if locked_until.0 > now.0 {
                    return Err(AuthError::Locked {
                        retry_at: locked_until,
                    });
                }
                state.global_locked_until = None;
                state.global_failures.clear();
            }
            let expired_lock = if let Some(failure) = state.failures.get(client) {
                if failure
                    .locked_until
                    .is_some_and(|locked_until| locked_until.0 > now.0)
                {
                    return Err(AuthError::Locked {
                        retry_at: failure.locked_until.unwrap_or(now),
                    });
                }
                failure.locked_until.is_some()
            } else {
                false
            };
            if expired_lock {
                state.failures.remove(client);
            }
            hash
        };

        let valid = verify_password(hash.clone(), password.to_string()).await?;
        let mut state = self.lock()?;
        if state.password_hash.as_deref() != Some(hash.as_str()) {
            return Err(AuthError::InvalidCredentials {
                attempts_remaining: 0,
            });
        }
        if valid {
            state.failures.remove(client);
            return self.issue_session(&mut state);
        }

        state
            .global_failures
            .retain(|at| now.0.saturating_sub(at.0) < GLOBAL_WINDOW_MS);
        state.global_failures.push(now);
        if state.global_failures.len() >= GLOBAL_MAX_FAILURES {
            let retry_at = Timestamp(now.0.saturating_add(LOCKOUT_MS));
            state.global_locked_until = Some(retry_at);
            // Rotated keys must not accumulate either.
            state.failures.clear();
            return Err(AuthError::Locked { retry_at });
        }
        let failure = state.failures.entry(client.to_string()).or_default();
        failure.attempts = failure.attempts.saturating_add(1);
        if failure.attempts >= MAX_LOGIN_ATTEMPTS {
            let retry_at = Timestamp(now.0.saturating_add(LOCKOUT_MS));
            failure.locked_until = Some(retry_at);
            Err(AuthError::Locked { retry_at })
        } else {
            Err(AuthError::InvalidCredentials {
                attempts_remaining: MAX_LOGIN_ATTEMPTS - failure.attempts,
            })
        }
    }

    /// Classify a valid machine or browser bearer token.
    pub fn authorize(&self, token: &str) -> Result<Option<CredentialKind>, AuthError> {
        if constant_time_eq(token, &self.machine_token) {
            return Ok(Some(CredentialKind::MachineToken));
        }
        let now = (self.clock)();
        let mut state = self.lock()?;
        state
            .sessions
            .retain(|session| session.expires_at.0 > now.0);
        Ok(state
            .sessions
            .iter()
            .any(|session| constant_time_eq(token, &session.token))
            .then_some(CredentialKind::BrowserSession))
    }

    /// Return expiry metadata for a valid browser session.
    pub fn session_info(&self, token: &str) -> Result<AuthSessionInfo, AuthError> {
        let now = (self.clock)();
        let mut state = self.lock()?;
        state
            .sessions
            .retain(|session| session.expires_at.0 > now.0);
        state
            .sessions
            .iter()
            .find(|session| constant_time_eq(token, &session.token))
            .map(|session| AuthSessionInfo {
                expires_at: session.expires_at,
            })
            .ok_or(AuthError::InvalidSession)
    }

    /// Revoke one browser session.
    pub fn logout(&self, token: &str) -> Result<(), AuthError> {
        let mut state = self.lock()?;
        let old_len = state.sessions.len();
        state
            .sessions
            .retain(|session| !constant_time_eq(token, &session.token));
        if state.sessions.len() == old_len {
            Err(AuthError::InvalidSession)
        } else {
            Ok(())
        }
    }

    /// Replace the owner password, revoke all sessions, and issue one replacement.
    pub async fn change_password(
        &self,
        session_token: &str,
        current_password: &str,
        new_password: &str,
    ) -> Result<AuthSession, AuthError> {
        self.session_info(session_token)?;
        validate_password(new_password)?;
        let old_hash = self
            .lock()?
            .password_hash
            .clone()
            .ok_or(AuthError::SetupRequired)?;
        if !verify_password(old_hash.clone(), current_password.to_string()).await? {
            return Err(AuthError::InvalidCredentials {
                attempts_remaining: 0,
            });
        }
        let new_hash = hash_password(new_password.to_string()).await?;
        let mut state = self.lock()?;
        if state.password_hash.as_deref() != Some(old_hash.as_str())
            || !state
                .sessions
                .iter()
                .any(|session| constant_time_eq(session_token, &session.token))
        {
            return Err(AuthError::InvalidSession);
        }
        persist_password_hash(&self.password_file, &new_hash)?;
        state.password_hash = Some(new_hash);
        state.sessions.clear();
        state.failures.clear();
        self.issue_session(&mut state)
    }

    fn issue_session(&self, state: &mut AuthState) -> Result<AuthSession, AuthError> {
        let expires_at = Timestamp((self.clock)().0.saturating_add(self.session_lifetime_ms));
        let token = random_hex_token()?;
        state.sessions.push(BrowserSession {
            token: token.clone(),
            expires_at,
        });
        Ok(AuthSession { token, expires_at })
    }

    fn lock(&self) -> Result<MutexGuard<'_, AuthState>, AuthError> {
        self.state.lock().map_err(|_| AuthError::StatePoisoned)
    }
}

fn validate_password(password: &str) -> Result<(), AuthError> {
    let length = password.len();
    if !(MIN_PASSWORD_BYTES..=MAX_PASSWORD_BYTES).contains(&length) {
        return Err(AuthError::InvalidPassword {
            reason: format!(
                "password must be between {MIN_PASSWORD_BYTES} and {MAX_PASSWORD_BYTES} bytes"
            ),
        });
    }
    Ok(())
}

async fn hash_password(password: String) -> Result<String, AuthError> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::encode_b64(&secure_random::<16>()?)
            .map_err(|error| AuthError::PasswordHash(error.to_string()))?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| AuthError::PasswordHash(error.to_string()))
    })
    .await
    .map_err(|error| AuthError::HashTask(error.to_string()))?
}

async fn verify_password(hash: String, password: String) -> Result<bool, AuthError> {
    tokio::task::spawn_blocking(move || {
        let parsed =
            PasswordHash::new(&hash).map_err(|error| AuthError::PasswordHash(error.to_string()))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
    .map_err(|error| AuthError::HashTask(error.to_string()))?
}

fn load_password_hash(path: &Path) -> Result<Option<String>, AuthError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(AuthError::PasswordFile {
                operation: "read",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let stored: PasswordFile =
        serde_json::from_str(&text).map_err(|error| AuthError::PasswordData {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    PasswordHash::new(&stored.password_hash).map_err(|error| AuthError::PasswordData {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    Ok(Some(stored.password_hash))
}

fn persist_password_hash(path: &Path, password_hash: &str) -> Result<(), AuthError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|source| password_file_error("create parent", path, source))?;
    }
    let suffix = random_hex_token()?;
    let temp = path.with_extension(format!("tmp-{suffix}"));
    let bytes = serde_json::to_vec(&PasswordFile {
        password_hash: password_hash.to_string(),
    })
    .map_err(|error| AuthError::PasswordData {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let write_result = options
        .open(&temp)
        .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()));
    if let Err(source) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(password_file_error("write", path, source));
    }
    if let Err(source) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(password_file_error("replace", path, source));
    }
    Ok(())
}

fn password_file_error(operation: &'static str, path: &Path, source: std::io::Error) -> AuthError {
    AuthError::PasswordFile {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn system_time() -> Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    Timestamp(i64::try_from(millis).unwrap_or(i64::MAX))
}
