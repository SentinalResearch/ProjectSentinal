//! The control's first owner password, set from the terminal (4.E §3). The
//! daemon only accepts it with the Machine token, which this same user can
//! read, so the owner never copies the token into a browser. The password is
//! read without echo and sent only to the daemon on loopback.

use super::{Io, SetupOptions, System};
use std::path::PathBuf;

/// Smallest password the daemon accepts.
const MIN_PASSWORD_BYTES: usize = 12;
const ATTEMPTS: usize = 3;

/// Ask for and set the owner password when the dashboard has none yet.
/// Returns whether a password is in place afterwards.
pub(super) fn ensure(
    options: &SetupOptions,
    token_file: PathBuf,
    system: &mut dyn System,
    io: &mut Io<'_>,
) -> bool {
    if system.password_needed() != Some(true) {
        return true;
    }
    if !options.interactive {
        let _ = writeln!(
            io.out,
            "  · owner password not set yet: run sentinal setup in a terminal to set it"
        );
        return false;
    }
    let _ = writeln!(
        io.out,
        "Set the owner password for the dashboard (at least {MIN_PASSWORD_BYTES} characters)."
    );
    for _ in 0..ATTEMPTS {
        let Some(first) = system.read_password("Owner password: ") else {
            break;
        };
        if first.len() < MIN_PASSWORD_BYTES {
            let _ = writeln!(
                io.out,
                "  The password must be at least {MIN_PASSWORD_BYTES} characters."
            );
            continue;
        }
        let Some(second) = system.read_password("Repeat it: ") else {
            break;
        };
        if first != second {
            let _ = writeln!(io.out, "  The passwords did not match.");
            continue;
        }
        return match system.set_password(&token_file, &first) {
            Ok(()) => {
                let _ = writeln!(io.out, "  ✓ owner password");
                true
            }
            Err(message) => {
                let _ = writeln!(io.out, "  The daemon refused the password ({message}).");
                false
            }
        };
    }
    let _ = writeln!(
        io.out,
        "  · owner password not set: run sentinal setup again to set it"
    );
    false
}
