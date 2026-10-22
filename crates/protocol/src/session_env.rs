//! The terminal environment an AUTO_WRAP or WATCH Session starts with.
//!
//! Those Sessions replace a shell the user was opening in their own terminal,
//! so the shell must see that terminal's environment (PATH from
//! `~/.zprofile`, `brew shellenv`, an active virtualenv, …), not the
//! daemon's service environment.

use crate::{ProtocolError, SessionOrigin};

/// Most variables one request may carry.
pub const MAX_ENV_VARS: usize = 4096;
/// Most bytes (names plus values) one request may carry.
pub const MAX_ENV_BYTES: usize = 512 * 1024;

/// Variables the daemon sets itself for every Session; a client's values are dropped.
pub const DAEMON_OWNED_VARS: &[&str] = &[
    "SENTINAL_SESSION_ID",
    "SENTINAL_AUTO_GUARD",
    "SENTINAL_OSC_NONCE",
    "TA_HOOK_DIR",
    "TA_USER_ZDOTDIR",
];

/// Whether a client-sent variable is one the daemon sets itself.
pub fn daemon_owned(name: &str) -> bool {
    DAEMON_OWNED_VARS.contains(&name)
}

/// Check a CreateSession `env`: only terminal-started origins, bounded, and
/// every entry a valid environment entry.
pub fn validate_session_env(
    origin: SessionOrigin,
    env: Option<&[(String, String)]>,
) -> Result<(), ProtocolError> {
    let Some(env) = env else {
        return Ok(());
    };
    let invalid = |reason: &str| Err(ProtocolError::InvalidSessionEnv(reason.to_string()));
    if !matches!(origin, SessionOrigin::AutoWrap | SessionOrigin::Watch) {
        return invalid("only AUTO_WRAP and WATCH Sessions take a terminal environment");
    }
    if env.len() > MAX_ENV_VARS {
        return invalid("too many variables");
    }
    let bytes: usize = env
        .iter()
        .map(|(name, value)| name.len() + value.len())
        .sum();
    if bytes > MAX_ENV_BYTES {
        return invalid("too large");
    }
    for (name, value) in env {
        if name.is_empty() || name.contains(['=', '\0']) {
            return invalid("a variable name is empty or contains '=' or NUL");
        }
        if value.contains('\0') {
            return invalid("a variable value contains NUL");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_session_env, MAX_ENV_VARS};
    use crate::SessionOrigin;

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.into(), value.into())
    }

    #[test]
    fn only_terminal_started_sessions_take_an_environment() {
        let env = [pair("PATH", "/opt/homebrew/bin:/usr/bin")];
        assert!(validate_session_env(SessionOrigin::AutoWrap, Some(&env)).is_ok());
        assert!(validate_session_env(SessionOrigin::Watch, Some(&env)).is_ok());
        assert!(validate_session_env(SessionOrigin::Ui, Some(&env)).is_err());
        assert!(validate_session_env(SessionOrigin::Ui, None).is_ok());
    }

    #[test]
    fn malformed_or_oversized_environments_are_refused() {
        let check =
            |env: &[(String, String)]| validate_session_env(SessionOrigin::AutoWrap, Some(env));
        assert!(check(&[pair("", "x")]).is_err());
        assert!(check(&[pair("A=B", "x")]).is_err());
        assert!(check(&[pair("A", "x\0y")]).is_err());
        let many: Vec<_> = (0..=MAX_ENV_VARS)
            .map(|n| pair(&format!("V{n}"), ""))
            .collect();
        assert!(check(&many).is_err());
        assert!(check(&[pair("BIG", &"x".repeat(600 * 1024))]).is_err());
        assert!(check(&[pair("EMPTY", "")]).is_ok());
    }
}
