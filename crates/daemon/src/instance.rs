//! This daemon install's random ID, and the commit it was built from.
//!
//! The ID lives with the Session database, so an update keeps it and a separate
//! install (a dev environment, a copied config) gets its own. The control uses it to
//! refuse a second install claiming a Machine that is already alive.

use crate::{auth::random_hex_token, private_fs::private_file_options};
use std::{fs, io::Write, path::Path};

const FILE: &str = "instance-id";

/// Failure reading or creating the instance ID.
#[derive(Debug, thiserror::Error)]
#[error("could not {operation} daemon instance ID in '{path}': {source}")]
pub struct InstanceError {
    operation: &'static str,
    path: String,
    source: std::io::Error,
}

/// This install's ID, created on first use and never changed afterwards.
pub fn load_or_create(data_dir: &Path) -> Result<String, InstanceError> {
    let path = data_dir.join(FILE);
    let failure = |operation| {
        let path = path.display().to_string();
        move |source| InstanceError {
            operation,
            path,
            source,
        }
    };
    match fs::read_to_string(&path) {
        Ok(text) if valid(text.trim()) => return Ok(text.trim().to_string()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failure("read")(error)),
    }
    fs::create_dir_all(data_dir).map_err(failure("create the directory for"))?;
    let id =
        random_hex_token().map_err(|error| failure("generate")(std::io::Error::other(error)))?;
    let mut file = private_file_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(failure("create"))?;
    file.write_all(id.as_bytes()).map_err(failure("write"))?;
    Ok(id)
}

fn valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// A release or dev build's short commit, with `-dirty` when it had
/// uncommitted changes. `None` for an unstamped development build.
pub fn build_commit() -> Option<String> {
    short_commit(option_env!("SENTINAL_GIT_COMMIT"))
}

fn short_commit(stamp: Option<&str>) -> Option<String> {
    let stamp = stamp?;
    let (hash, dirty) = stamp
        .strip_suffix("-dirty")
        .map_or((stamp, false), |hash| (hash, true));
    if hash.len() < 7 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!(
        "{}{}",
        &hash[..7],
        if dirty { "-dirty" } else { "" }
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_stable_for_one_install_and_differs_between_installs() {
        let root = std::env::temp_dir().join(format!("sentinal-instance-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let first = load_or_create(&root.join("a")).unwrap();
        assert_eq!(load_or_create(&root.join("a")).unwrap(), first);
        assert_ne!(load_or_create(&root.join("b")).unwrap(), first);
        fs::write(root.join("a").join(FILE), "not valid!\n").unwrap();
        assert_ne!(load_or_create(&root.join("a")).unwrap(), "not valid!");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn commits_are_shortened_and_marked_when_dirty() {
        assert_eq!(
            short_commit(Some("27357f9abcdef")).as_deref(),
            Some("27357f9")
        );
        assert_eq!(
            short_commit(Some("27357f9abcdef-dirty")).as_deref(),
            Some("27357f9-dirty")
        );
        assert_eq!(short_commit(Some("unknown")), None);
        assert_eq!(short_commit(None), None);
    }
}
