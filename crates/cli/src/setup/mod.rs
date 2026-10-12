//! `sentinal setup` (4.E §3): choose this Machine's role, preview every change,
//! and apply only after the owner agrees. Re-running finishes what is missing
//! and changes nothing that is already right. The WORKER role hands the chosen
//! control URL to the existing one-time-code `connect` flow.

pub mod config;
mod control;
mod system;
mod worker;

use std::{
    fs,
    io::{BufRead, Write},
    path::PathBuf,
};
pub use system::{RealSystem, ServeRoute, System, TailscaleSelf};

/// Exit codes (`docs/CLI.md`).
pub mod exit {
    /// Done, or nothing to do.
    pub const OK: i32 = 0;
    /// A step failed; changed files were restored.
    pub const FAILED: i32 = 1;
    /// Usage error or cancelled; nothing changed.
    pub const CANCELLED: i32 = 2;
    /// Already configured differently.
    pub const CONFLICT: i32 = 5;
    /// A prerequisite (Tailscale) is missing.
    pub const PREREQUISITE: i32 = 6;
}

/// The role chosen on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupRole {
    /// Serve the dashboard and packages.
    Control,
    /// Run Sessions for a control.
    Worker,
}

/// Inputs for one run.
pub struct SetupOptions {
    /// `--role`; asked when absent.
    pub role: Option<SetupRole>,
    /// `--name`: the control's display name, or the worker's.
    pub name: Option<String>,
    /// `--control-url` for the WORKER role.
    pub control_url: Option<String>,
    /// `--yes`: apply the preview without asking.
    pub yes: bool,
    /// Whether stdin is a terminal the owner can answer on.
    pub interactive: bool,
    /// Installed program prefix.
    pub prefix: PathBuf,
    /// Daemon config directory.
    pub config_dir: PathBuf,
    /// Home directory (default data directory).
    pub home: PathBuf,
    /// Login name (for linger).
    pub user: String,
    /// Installed release version.
    pub version: String,
}

/// What the caller does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Finished with this exit code.
    Exit(i32),
    /// Pair with this control using a one-time code.
    Connect {
        /// Validated control origin.
        control_url: String,
        /// Proposed display name.
        name: Option<String>,
    },
}

struct Io<'a> {
    input: &'a mut dyn BufRead,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Io<'_> {
    /// One answer line; `None` on end of input or `q`.
    fn ask(&mut self, prompt: &str) -> Option<String> {
        let _ = write!(self.out, "{prompt}");
        let _ = self.out.flush();
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                let answer = line.trim().to_string();
                (answer != "q").then_some(answer)
            }
        }
    }

    fn cancelled(&mut self) -> Outcome {
        let _ = writeln!(self.err, "sentinal setup: cancelled; nothing was changed");
        Outcome::Exit(exit::CANCELLED)
    }

    fn fail(&mut self, code: i32, message: &str) -> Outcome {
        let _ = writeln!(self.err, "sentinal setup: {message}");
        Outcome::Exit(code)
    }
}

/// Run setup.
pub fn run(
    options: SetupOptions,
    system: &mut dyn System,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Outcome {
    let mut io = Io { input, out, err };
    let answered = match options.role {
        Some(SetupRole::Control) => options.yes,
        Some(SetupRole::Worker) => options.control_url.is_some(),
        None => false,
    };
    if !options.interactive && !answered {
        return io.fail(
            exit::CANCELLED,
            "no terminal to ask on: pass --role control --yes, or --role worker --control-url URL --code-stdin",
        );
    }
    let config_path = options.config_dir.join("daemon.toml");
    let existing = fs::read_to_string(&config_path).ok();
    let is_control = existing
        .as_deref()
        .and_then(|text| toml::from_str::<toml::Value>(text).ok())
        .and_then(|value| value.get("control")?.get("enabled")?.as_bool())
        == Some(true);

    let mut skip_discovery = options.control_url.is_some();
    let role = match options.role {
        Some(role) => role,
        None => {
            let _ = writeln!(
                io.out,
                "Sentinal {}\n\nWhat is this Machine?",
                options.version
            );
            let _ = writeln!(io.out, "  1) Create my Sentinal control      (this Machine serves the dashboard and packages)");
            let _ = writeln!(
                io.out,
                "  2) Connect to my existing Sentinal (this Machine runs Sessions for a control)"
            );
            let _ = writeln!(
                io.out,
                "  3) Advanced: enter a control URL and use a one-time code"
            );
            match io.ask("Choose 1-3 (q to quit): ").as_deref() {
                Some("1") => SetupRole::Control,
                Some("2") => SetupRole::Worker,
                Some("3") => {
                    skip_discovery = true;
                    SetupRole::Worker
                }
                _ => return io.cancelled(),
            }
        }
    };
    match role {
        SetupRole::Control => control::control(&options, existing.as_deref(), system, &mut io),
        SetupRole::Worker if is_control => io.fail(
            exit::CONFLICT,
            &format!(
                "this Machine is a control ({} has [control] enabled); a control does not pair with another control",
                config_path.display()
            ),
        ),
        SetupRole::Worker => worker::worker(&options, skip_discovery, system, &mut io),
    }
}
