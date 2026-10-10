//! The version this binary reports: the release it was built for, or a
//! clearly labelled development build. `Cargo.toml` stays at 0.0.1; release
//! builds set `SENTINAL_RELEASE_VERSION` and `SENTINAL_GIT_COMMIT`.

use std::sync::LazyLock;

/// For example `0.1.16 (ac92ae5)` or `0.0.1 (development build)`.
pub static VERSION: LazyLock<String> = LazyLock::new(|| {
    describe(
        option_env!("SENTINAL_RELEASE_VERSION"),
        option_env!("SENTINAL_GIT_COMMIT"),
        env!("CARGO_PKG_VERSION"),
    )
});

fn describe(release: Option<&str>, commit: Option<&str>, package: &str) -> String {
    match release.filter(|release| !release.is_empty()) {
        Some(release) => match commit.filter(|commit| commit.len() >= 7) {
            Some(commit) => format!("{release} ({})", &commit[..7]),
            None => release.to_string(),
        },
        None => format!("{package} (development build)"),
    }
}

#[cfg(test)]
mod tests {
    use super::describe;

    #[test]
    fn release_builds_name_the_release_and_short_commit() {
        assert_eq!(
            describe(Some("0.1.16"), Some("ac92ae5f00d1"), "0.0.1"),
            "0.1.16 (ac92ae5)"
        );
        assert_eq!(describe(Some("0.1.16"), None, "0.0.1"), "0.1.16");
    }

    #[test]
    fn unstamped_builds_say_they_are_development_builds() {
        assert_eq!(describe(None, None, "0.0.1"), "0.0.1 (development build)");
        assert_eq!(
            describe(Some(""), Some("abc"), "0.0.1"),
            "0.0.1 (development build)"
        );
    }
}
