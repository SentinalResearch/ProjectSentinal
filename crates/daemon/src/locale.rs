//! A UTF-8 locale for Sessions whose environment names none. sentinald runs
//! as a service without `LANG`, and a shell without a locale treats every
//! byte above 0x7F as a separate character: `ls` prints `?` for accented
//! names, readline breaks on emoji, and Python defaults to ASCII.

use std::path::Path;

/// Files that name the system locale on Linux, in the order they are read.
const SYSTEM_LOCALE_FILES: &[&str] = &["/etc/default/locale", "/etc/locale.conf"];

/// `LANG` to add to a Session, or `None` when `lookup` already finds a locale.
pub fn session_lang(lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    session_lang_with(lookup, cfg!(target_os = "macos"), |path| {
        std::fs::read_to_string(path).ok()
    })
}

fn session_lang_with(
    lookup: impl Fn(&str) -> Option<String>,
    macos: bool,
    read: impl Fn(&Path) -> Option<String>,
) -> Option<String> {
    let set = |name: &str| lookup(name).is_some_and(|value| !value.is_empty());
    if set("LC_ALL") || set("LANG") {
        return None;
    }
    if macos {
        return Some("en_US.UTF-8".into());
    }
    let system = SYSTEM_LOCALE_FILES
        .iter()
        .find_map(|path| read(Path::new(path)).and_then(|text| lang_in(&text)));
    Some(system.unwrap_or_else(|| "C.UTF-8".into()))
}

/// The `LANG=` value of a shell-style locale file.
fn lang_in(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("LANG="))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
        .rfind(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::session_lang_with;
    use std::path::Path;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    fn files<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&Path) -> Option<String> + 'a {
        move |path| {
            pairs
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| (*text).to_string())
        }
    }

    #[test]
    fn an_existing_locale_is_kept() {
        let none = files(&[]);
        assert_eq!(
            session_lang_with(env(&[("LANG", "de_DE.UTF-8")]), false, &none),
            None
        );
        assert_eq!(
            session_lang_with(env(&[("LC_ALL", "C.UTF-8")]), true, &none),
            None
        );
        assert_eq!(
            session_lang_with(env(&[("LANG", "")]), true, &none),
            Some("en_US.UTF-8".into()),
            "an empty LANG names no locale"
        );
    }

    #[test]
    fn linux_uses_the_system_locale_file() {
        let debian = files(&[(
            "/etc/default/locale",
            "# generated\nLANG=\"en_GB.UTF-8\"\nLANGUAGE=en_GB:en\n",
        )]);
        assert_eq!(
            session_lang_with(env(&[]), false, debian),
            Some("en_GB.UTF-8".into())
        );
        let fedora = files(&[("/etc/locale.conf", "LANG=fr_FR.UTF-8\n")]);
        assert_eq!(
            session_lang_with(env(&[]), false, fedora),
            Some("fr_FR.UTF-8".into())
        );
    }

    #[test]
    fn otherwise_a_utf8_default_per_platform() {
        let empty = files(&[("/etc/default/locale", "# nothing here\n")]);
        assert_eq!(
            session_lang_with(env(&[]), false, &empty),
            Some("C.UTF-8".into())
        );
        assert_eq!(
            session_lang_with(env(&[]), true, &empty),
            Some("en_US.UTF-8".into())
        );
    }
}
