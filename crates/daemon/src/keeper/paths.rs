//! Where keeper sockets live and who may talk to them
//! (`docs/R1_KEEPER_DESIGN.md` §4 "Socket location", §7).

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// Longest socket path the platforms take (`sun_path`: 104 on macOS).
pub const MAX_SOCKET_PATH: usize = 100;

/// The directory for this daemon's keeper sockets: the per-user runtime
/// directory (Linux) or temporary directory (macOS), else the data
/// directory. Named after the data directory, so a development daemon and
/// the installed one never share sockets.
pub fn socket_dir(data_dir: &Path) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|_| cfg!(target_os = "linux"))
        .or_else(|| std::env::var_os("TMPDIR").filter(|_| cfg!(target_os = "macos")))
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir());
    match base {
        Some(base) => base.join(format!("sentinal-keepers-{}", short_hash(data_dir))),
        None => data_dir.join("keepers"),
    }
}

fn short_hash(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(path.as_os_str().as_encoded_bytes());
    super::protocol::hex(&digest[..4])
}

/// The socket for one Session, if its path fits the platform limit.
pub fn socket_path(dir: &Path, session_id: u64) -> Option<PathBuf> {
    let path = dir.join(format!("{session_id}.sock"));
    (path.as_os_str().len() <= MAX_SOCKET_PATH).then_some(path)
}

/// Create `dir` for this user only (`0700`), or check an existing one is.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    check_private(dir, 0o700)
}

/// `path` is owned by this user and not open to group or others.
pub fn check_private(path: &Path, _mode: u32) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(refused(path, "is a symbolic link"));
    }
    if metadata.uid() != nix::unistd::geteuid().as_raw() {
        return Err(refused(path, "belongs to another user"));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(refused(path, "is open to other users"));
    }
    Ok(())
}

fn refused(path: &Path, why: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!("{} {why}", path.display()),
    )
}

/// The user ID of the process at the other end of `stream` (Q1).
pub fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    #[cfg(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        let (uid, _) = nix::unistd::getpeereid(stream).map_err(std::io::Error::from)?;
        Ok(uid.as_raw())
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
        let credentials = getsockopt(stream, PeerCredentials).map_err(std::io::Error::from)?;
        Ok(credentials.uid())
    }
}

/// The peer runs as this user; anything else is refused.
pub fn check_peer(stream: &UnixStream) -> std::io::Result<()> {
    if peer_uid(stream)? == nix::unistd::geteuid().as_raw() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "a process of another user connected",
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("k-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn two_daemons_never_share_a_socket_directory() {
        let a = socket_dir(Path::new("/home/me/.local/share/terminal-agent"));
        let b = socket_dir(Path::new("/home/me/.local/share/sentinal-dev-qa/data"));
        assert_ne!(a, b);
        assert_eq!(
            a,
            socket_dir(Path::new("/home/me/.local/share/terminal-agent"))
        );
    }

    #[test]
    fn a_socket_path_too_long_for_the_platform_is_refused() {
        assert!(socket_path(Path::new("/tmp/k"), 42).is_some());
        let long = PathBuf::from(format!("/{}", "d".repeat(120)));
        assert!(socket_path(&long, 42).is_none());
    }

    #[test]
    fn only_a_private_directory_of_this_user_is_used() {
        let dir = temp("private");
        ensure_private_dir(&dir).unwrap();
        check_private(&dir, 0o700).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check_private(&dir, 0o700).is_err());
        ensure_private_dir(&dir).unwrap();
        let link = temp("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(check_private(&link, 0o700).is_err());
        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_peer_of_a_local_socket_is_this_user() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a).unwrap(), nix::unistd::geteuid().as_raw());
        check_peer(&a).unwrap();
    }
}
