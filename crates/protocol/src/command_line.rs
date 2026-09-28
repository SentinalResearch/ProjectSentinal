//! Rendering a structured Command argv as one interactive shell input line.
//!
//! This is the only approved way to turn Watch argv into shell text. The line
//! is typed into an interactive shell's line editor, so it must be safe at
//! two levels:
//!
//! - **Shell parsing:** every argument is quoted so it stays one word, and the
//!   executable is always quoted so aliases, keywords, and `NAME=value`
//!   assignments cannot change which program runs.
//! - **Line editing:** the rendered text contains only printable ASCII (and,
//!   for fish, printable UTF-8). Control bytes are written as escapes, so no
//!   byte can act as an editor key such as Ctrl-C, Ctrl-U, Tab, or Enter.
//!
//! The rendered line is capped below the 1024-byte canonical-input limit of
//! macOS terminals so a line typed before the editor enters raw mode can
//! never be truncated into a different command.

use crate::{message::validate_argv, ProtocolError};

/// Maximum bytes in a rendered initial Command line, excluding the final Enter.
pub const MAX_RENDERED_COMMAND_BYTES: usize = 1_000;

/// Shell language used to quote a rendered Command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellDialect {
    /// GNU bash.
    Bash,
    /// zsh.
    Zsh,
    /// fish.
    Fish,
}

impl ShellDialect {
    /// Dialect for a shell executable path, or `None` for unsupported shells.
    pub fn from_shell_path(shell: &str) -> Option<Self> {
        match shell.rsplit('/').next().unwrap_or(shell) {
            "bash" => Some(ShellDialect::Bash),
            "zsh" => Some(ShellDialect::Zsh),
            "fish" => Some(ShellDialect::Fish),
            _ => None,
        }
    }
}

/// Render validated argv as one line of shell input, without the final Enter.
pub fn render_command_line(
    argv: &[String],
    dialect: ShellDialect,
) -> Result<String, ProtocolError> {
    validate_argv(argv)?;
    let words: Vec<String> = argv
        .iter()
        .enumerate()
        .map(|(index, argument)| match dialect {
            ShellDialect::Bash | ShellDialect::Zsh => posix_word(argument, index == 0),
            ShellDialect::Fish => fish_word(argument, index == 0),
        })
        .collect();
    let line = words.join(" ");
    if line.len() > MAX_RENDERED_COMMAND_BYTES {
        return Err(ProtocolError::FieldTooLong {
            field: "rendered initial command",
            actual: line.len(),
            maximum: MAX_RENDERED_COMMAND_BYTES,
        });
    }
    Ok(line)
}

/// Characters that never need quoting in any supported shell, anywhere in a word.
fn is_plain(argument: &str) -> bool {
    !argument.is_empty()
        && !argument.starts_with('=')
        && argument
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:=@,+%".contains(&byte))
}

fn posix_word(argument: &str, force_quotes: bool) -> String {
    if !force_quotes && is_plain(argument) {
        return argument.to_string();
    }
    if argument.bytes().all(|byte| (0x20..0x7f).contains(&byte)) {
        return format!("'{}'", argument.replace('\'', r"'\''"));
    }
    // ANSI-C quoting keeps control and non-ASCII bytes out of the line editor.
    // `'` and `!` are hex-escaped so bash history expansion never sees them.
    let mut word = String::from("$'");
    for byte in argument.bytes() {
        match byte {
            b'\\' => word.push_str(r"\\"),
            b'\'' | b'!' => word.push_str(&format!(r"\x{byte:02x}")),
            0x20..=0x7e => word.push(char::from(byte)),
            _ => word.push_str(&format!(r"\x{byte:02x}")),
        }
    }
    word.push('\'');
    word
}

fn fish_word(argument: &str, force_quotes: bool) -> String {
    if !force_quotes && is_plain(argument) {
        return argument.to_string();
    }
    let mut word = String::new();
    let mut quoted = false;
    for character in argument.chars() {
        if character.is_control() {
            // Escapes are only interpreted outside fish single quotes.
            if quoted {
                word.push('\'');
                quoted = false;
            }
            let code = u32::from(character);
            if code < 0x80 {
                word.push_str(&format!(r"\x{code:02x}"));
            } else {
                word.push_str(&format!(r"\u{code:04x}"));
            }
            continue;
        }
        if !quoted {
            word.push('\'');
            quoted = true;
        }
        match character {
            '\\' => word.push_str(r"\\"),
            '\'' => word.push_str(r"\'"),
            c => word.push(c),
        }
    }
    if word.is_empty() {
        word.push_str("''");
    } else if quoted {
        word.push('\'');
    }
    word
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn plain_arguments_stay_readable_but_executable_is_quoted() {
        let line = render_command_line(
            &argv(&["python", "train.py", "--lr=3e-4"]),
            ShellDialect::Zsh,
        )
        .unwrap();
        assert_eq!(line, "'python' train.py --lr=3e-4");
    }

    #[test]
    fn assignment_like_executable_is_not_an_assignment() {
        let line = render_command_line(&argv(&["FOO=bar", "x"]), ShellDialect::Bash).unwrap();
        assert_eq!(line, "'FOO=bar' x");
    }

    #[test]
    fn zsh_equals_expansion_is_quoted() {
        let line = render_command_line(&argv(&["echo", "=ls"]), ShellDialect::Zsh).unwrap();
        assert_eq!(line, "'echo' '=ls'");
    }

    #[test]
    fn control_bytes_never_appear_in_rendered_text() {
        let hostile = argv(&["sh", "\x03rm -rf ~\r", "a\nb", "\t", "\x1b[A", "\u{9b}x"]);
        for dialect in [ShellDialect::Bash, ShellDialect::Zsh, ShellDialect::Fish] {
            let line = render_command_line(&hostile, dialect).unwrap();
            assert!(
                line.chars().all(|c| !c.is_control()),
                "{dialect:?} rendered a control character: {line:?}"
            );
        }
    }

    #[test]
    fn empty_argv_and_executable_are_rejected() {
        assert_eq!(
            render_command_line(&[], ShellDialect::Bash),
            Err(ProtocolError::EmptyCommandArgv)
        );
        assert_eq!(
            render_command_line(&argv(&[""]), ShellDialect::Fish),
            Err(ProtocolError::EmptyCommandExecutable)
        );
    }

    #[test]
    fn oversized_rendering_is_rejected() {
        let long = "x".repeat(MAX_RENDERED_COMMAND_BYTES);
        let error = render_command_line(&argv(&["echo", &long]), ShellDialect::Zsh).unwrap_err();
        assert!(error.to_string().contains("rendered initial command"));
    }

    #[test]
    fn dialect_is_chosen_by_executable_name() {
        assert_eq!(
            ShellDialect::from_shell_path("/bin/zsh"),
            Some(ShellDialect::Zsh)
        );
        assert_eq!(
            ShellDialect::from_shell_path("/opt/homebrew/bin/fish"),
            Some(ShellDialect::Fish)
        );
        assert_eq!(
            ShellDialect::from_shell_path("bash"),
            Some(ShellDialect::Bash)
        );
        assert_eq!(ShellDialect::from_shell_path("/bin/sh"), None);
    }
}
