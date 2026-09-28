//! Where the private local socket lives, shared by `sentinald` and reporters.
//!
//! Pure path selection only; the daemon enforces ownership and permissions.

use std::path::{Path, PathBuf};

/// Default daemon data directory, relative to the user's home directory.
pub const DEFAULT_DATA_DIR: &str = ".local/share/terminal-agent";

/// Environment variable that points reporters at a non-default socket.
pub const SOCKET_ENV: &str = "SENTINAL_SOCKET";

/// Linux prefers `$XDG_RUNTIME_DIR/sentinal/sentinald.sock`; macOS and Linux
/// without an absolute runtime directory use `<data_dir>/run/sentinald.sock`.
pub fn local_socket_path(
    data_dir: &Path,
    xdg_runtime_dir: Option<&Path>,
    target_is_macos: bool,
) -> PathBuf {
    if !target_is_macos {
        if let Some(runtime) = xdg_runtime_dir.filter(|path| path.is_absolute()) {
            return runtime.join("sentinal/sentinald.sock");
        }
    }
    data_dir.join("run/sentinald.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_prefers_an_absolute_runtime_directory() {
        let data = Path::new("/home/u/.local/share/terminal-agent");
        assert_eq!(
            local_socket_path(data, Some(Path::new("/run/user/1000")), false),
            Path::new("/run/user/1000/sentinal/sentinald.sock")
        );
        assert_eq!(
            local_socket_path(data, Some(Path::new("relative")), false),
            data.join("run/sentinald.sock")
        );
    }

    #[test]
    fn macos_always_uses_the_data_directory() {
        let data = Path::new("/Users/u/.local/share/terminal-agent");
        assert_eq!(
            local_socket_path(data, Some(Path::new("/run/user/501")), true),
            data.join("run/sentinald.sock")
        );
    }
}
