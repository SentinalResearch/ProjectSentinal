//! The environment a terminal-started Session should begin with: this
//! terminal's own, so the wrapped shell finds the same PATH (Homebrew,
//! conda, a virtualenv, …) the unwrapped one would have.

use protocol::session_env::{daemon_owned, MAX_ENV_BYTES, MAX_ENV_VARS};

/// This process's environment, or `None` when it cannot be sent (the daemon
/// then uses its own environment, as before).
pub fn terminal_env() -> Option<Vec<(String, String)>> {
    from_vars(std::env::vars_os().map(|(name, value)| (name.into_string(), value.into_string())))
}

fn from_vars<I>(vars: I) -> Option<Vec<(String, String)>>
where
    I: IntoIterator<
        Item = (
            Result<String, std::ffi::OsString>,
            Result<String, std::ffi::OsString>,
        ),
    >,
{
    let env: Vec<(String, String)> = vars
        .into_iter()
        // A variable that is not UTF-8 cannot travel in the JSON request.
        .filter_map(|(name, value)| Some((name.ok()?, value.ok()?)))
        .filter(|(name, _)| !daemon_owned(name))
        .filter(|(name, value)| !name.is_empty() && !name.contains('=') && !value.contains('\0'))
        .collect();
    let bytes: usize = env
        .iter()
        .map(|(name, value)| name.len() + value.len())
        .sum();
    (env.len() <= MAX_ENV_VARS && bytes <= MAX_ENV_BYTES).then_some(env)
}

#[cfg(test)]
mod tests {
    use super::from_vars;

    fn ok(
        name: &str,
        value: &str,
    ) -> (
        Result<String, std::ffi::OsString>,
        Result<String, std::ffi::OsString>,
    ) {
        (Ok(name.into()), Ok(value.into()))
    }

    #[test]
    fn sends_the_terminal_environment_without_daemon_owned_variables() {
        let env = from_vars([
            ok("PATH", "/opt/homebrew/bin:/usr/bin"),
            ok("VIRTUAL_ENV", "/home/me/env-ai"),
            ok("SENTINAL_SESSION_ID", "4"),
            ok("SENTINAL_AUTO_GUARD", "1"),
            (Ok("BINARY".into()), Err(std::ffi::OsString::new())),
        ])
        .unwrap();
        assert_eq!(
            env,
            vec![
                ("PATH".into(), "/opt/homebrew/bin:/usr/bin".into()),
                ("VIRTUAL_ENV".into(), "/home/me/env-ai".into()),
            ]
        );
    }

    #[test]
    fn an_oversized_environment_falls_back_to_the_daemon_one() {
        assert_eq!(from_vars([ok("HUGE", &"x".repeat(600 * 1024))]), None);
    }
}
