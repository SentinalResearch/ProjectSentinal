//! `sentinal agent-skill --install`: add or update the agent skill in this
//! project's instructions file (Milestone 6.5).
//!
//! It never writes silently. It prints the exact change and writes only after
//! the owner types `y` on a real terminal. Run without a terminal, as an
//! agent's tool call would be, it shows the change and writes nothing. There
//! is deliberately no `--yes`. Only the text between the markers is touched.

use std::{
    io::{BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

/// Opens the managed section.
pub const BEGIN: &str = "<!-- sentinal:agent-skill -->";
/// Closes the managed section.
pub const END: &str = "<!-- /sentinal:agent-skill -->";

/// What `install` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The owner confirmed and the file was written.
    Written(PathBuf),
    /// The file already holds this skill.
    UpToDate(PathBuf),
    /// The owner answered no.
    Declined(PathBuf),
    /// No terminal to ask on: nothing was written.
    NeedsOwner(PathBuf),
}

/// The file to change: `file`, else `AGENTS.md`, else `CLAUDE.md`, else a new
/// `AGENTS.md`, all in `dir`.
pub fn target(dir: &Path, file: Option<&Path>) -> PathBuf {
    if let Some(file) = file {
        return file.to_path_buf();
    }
    let agents = dir.join("AGENTS.md");
    let claude = dir.join("CLAUDE.md");
    if !agents.exists() && claude.exists() {
        claude
    } else {
        agents
    }
}

/// `original` with the managed section set to `skill`: replaced in place when
/// the markers exist, appended at the end otherwise.
pub fn with_skill(original: &str, skill: &str) -> String {
    let skill = skill.trim_end_matches('\n');
    let block = format!("{BEGIN}\n{skill}\n{END}");
    if let (Some(start), Some(end)) = (original.find(BEGIN), original.find(END)) {
        if start < end {
            return format!(
                "{}{block}{}",
                &original[..start],
                &original[end + END.len()..]
            );
        }
    }
    let mut text = original.to_string();
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(&block);
    text.push('\n');
    text
}

/// A line diff of the changed middle: `-` removed, `+` added.
pub fn diff(old: &str, new: &str) -> String {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = String::new();
    for line in &old[prefix..old.len() - suffix] {
        out.push_str(&format!("-{line}\n"));
    }
    for line in &new[prefix..new.len() - suffix] {
        out.push_str(&format!("+{line}\n"));
    }
    out
}

/// Show the change, ask `confirm` (`None` = no terminal), and write on yes.
pub fn install(
    dir: &Path,
    file: Option<&Path>,
    skill: &str,
    confirm: &mut dyn FnMut(&str) -> Option<bool>,
    out: &mut dyn Write,
) -> std::io::Result<Outcome> {
    let path = target(dir, file);
    let original = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let updated = with_skill(&original, skill);
    if updated == original {
        return Ok(Outcome::UpToDate(path));
    }
    let change = diff(&original, &updated);
    writeln!(out, "Change to {}:\n", path.display())?;
    out.write_all(change.as_bytes())?;
    match confirm(&change) {
        Some(true) => {
            std::fs::write(&path, updated)?;
            Ok(Outcome::Written(path))
        }
        Some(false) => Ok(Outcome::Declined(path)),
        None => Ok(Outcome::NeedsOwner(path)),
    }
}

/// Run `sentinal agent-skill --install` in the current directory.
pub fn run(file: Option<&Path>, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    let dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(error) => {
            let _ = writeln!(err, "sentinal agent-skill: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut ask = |_: &str| -> Option<bool> {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() || !std::io::stdout().is_terminal() {
            return None;
        }
        print!("\nWrite this change? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        stdin.lock().read_line(&mut answer).ok()?;
        Some(matches!(answer.trim(), "y" | "Y" | "yes"))
    };
    match install(&dir, file, crate::AGENT_SKILL, &mut ask, out) {
        Ok(Outcome::Written(path)) => {
            let _ = writeln!(out, "Updated {}.", path.display());
            ExitCode::SUCCESS
        }
        Ok(Outcome::UpToDate(path)) => {
            let _ = writeln!(out, "{} already has this skill.", path.display());
            ExitCode::SUCCESS
        }
        Ok(Outcome::Declined(_)) => {
            let _ = writeln!(out, "Nothing written.");
            ExitCode::SUCCESS
        }
        Ok(Outcome::NeedsOwner(_)) => {
            let _ = writeln!(
                err,
                "\nNothing written: the owner must run `sentinal agent-skill --install` \
                 in their own terminal to confirm."
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            let _ = writeln!(err, "sentinal agent-skill: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_without_a_trailing_newline_gets_a_blank_line_first() {
        assert_eq!(with_skill("x", "S"), format!("x\n\n{BEGIN}\nS\n{END}\n"));
        assert_eq!(with_skill("", "S\n"), format!("{BEGIN}\nS\n{END}\n"));
    }

    #[test]
    fn text_after_the_section_is_kept() {
        let before = format!("a\n{BEGIN}\nold\n{END}\nb\n");
        assert_eq!(
            with_skill(&before, "new"),
            format!("a\n{BEGIN}\nnew\n{END}\nb\n")
        );
        assert_eq!(diff(&before, &with_skill(&before, "new")), "-old\n+new\n");
    }
}
