//! Quoting/injection tests for rendered Watch initial Commands.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{render_command_line, ShellDialect};

fn hostile() -> Vec<String> {
    [
        "printf",
        "%s|",
        "$(touch /tmp/sentinal-pwned)",
        "`id`",
        "a; rm -rf ~",
        "line one\nline two",
        "it's",
        "\"double\"",
        "",
        "  spaced  ",
        "tab\there",
        "back\\slash\\",
        "!history !!",
        "*.py",
        "~root",
        "=ls",
        "FOO=bar",
        "héllo 世界 🚀",
        "\u{3}\u{15}\u{1b}[A\r",
        "\u{9b}c1",
        "{a,b}",
        "#comment",
        "&& | > < &",
    ]
    .iter()
    .map(|value| (*value).to_string())
    .collect()
}

/// Independent decoder for the POSIX forms the renderer may emit.
fn decode_posix(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut words = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let mut word = Vec::new();
        while index < bytes.len() && bytes[index] != b' ' {
            match bytes[index] {
                b'\'' => {
                    index += 1;
                    while bytes[index] != b'\'' {
                        word.push(bytes[index]);
                        index += 1;
                    }
                    index += 1;
                }
                b'\\' => {
                    word.push(bytes[index + 1]);
                    index += 2;
                }
                b'$' if bytes.get(index + 1) == Some(&b'\'') => {
                    index += 2;
                    while bytes[index] != b'\'' {
                        if bytes[index] == b'\\' {
                            if bytes[index + 1] == b'x' {
                                let hex =
                                    std::str::from_utf8(&bytes[index + 2..index + 4]).unwrap();
                                word.push(u8::from_str_radix(hex, 16).unwrap());
                                index += 4;
                            } else {
                                word.push(bytes[index + 1]);
                                index += 2;
                            }
                        } else {
                            word.push(bytes[index]);
                            index += 1;
                        }
                    }
                    index += 1;
                }
                other => {
                    assert!(
                        other.is_ascii_alphanumeric() || b"-_./:=@,+%".contains(&other),
                        "unquoted special byte {other:#x} in {line:?}"
                    );
                    word.push(other);
                    index += 1;
                }
            }
        }
        words.push(String::from_utf8(word).unwrap());
        index += 1;
    }
    words
}

/// Independent decoder for the fish forms the renderer may emit.
fn decode_fish(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut words = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let mut word = String::new();
        while index < chars.len() && chars[index] != ' ' {
            match chars[index] {
                '\'' => {
                    index += 1;
                    while chars[index] != '\'' {
                        if chars[index] == '\\' && matches!(chars[index + 1], '\\' | '\'') {
                            word.push(chars[index + 1]);
                            index += 2;
                        } else {
                            word.push(chars[index]);
                            index += 1;
                        }
                    }
                    index += 1;
                }
                '\\' => {
                    let digits = if chars[index + 1] == 'x' { 2 } else { 4 };
                    let hex: String = chars[index + 2..index + 2 + digits].iter().collect();
                    word.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                    index += 2 + digits;
                }
                other => {
                    word.push(other);
                    index += 1;
                }
            }
        }
        words.push(word);
        index += 1;
    }
    words
}

#[test]
fn hostile_argv_round_trips_as_data_in_every_dialect() {
    let argv = hostile();
    for dialect in [ShellDialect::Bash, ShellDialect::Zsh] {
        let line = render_command_line(&argv, dialect).unwrap();
        assert!(line.is_ascii(), "{dialect:?}: {line}");
        assert!(!line.contains('\n') && !line.contains('\r'));
        assert_eq!(decode_posix(&line), argv, "{dialect:?}: {line}");
    }
    let line = render_command_line(&argv, ShellDialect::Fish).unwrap();
    assert!(line.chars().all(|c| !c.is_control()), "{line}");
    assert_eq!(decode_fish(&line), argv, "fish: {line}");
}

#[test]
fn bash_history_expansion_characters_never_appear_outside_single_quotes() {
    let line = render_command_line(&hostile(), ShellDialect::Bash).unwrap();
    let mut in_quotes = false;
    let mut escaped = false;
    for byte in line.bytes() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' && !in_quotes {
            escaped = true;
        } else if byte == b'\'' {
            in_quotes = !in_quotes;
        } else if byte == b'!' {
            assert!(in_quotes, "unquoted ! in {line}");
        }
    }
}

#[test]
#[ignore = "runs real bash, zsh, and fish when installed"]
fn shell_quoting_integration() {
    let mut argv = hostile();
    argv[1] = "%s\\0".into();
    let mut expected = Vec::new();
    for argument in &argv[2..] {
        expected.extend_from_slice(argument.as_bytes());
        expected.push(0);
    }
    for (shell, dialect) in [
        ("bash", ShellDialect::Bash),
        ("zsh", ShellDialect::Zsh),
        ("fish", ShellDialect::Fish),
    ] {
        let line = render_command_line(&argv, dialect).unwrap();
        let output = match std::process::Command::new(shell)
            .arg("-c")
            .arg(&line)
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                eprintln!("skipping {shell}: {error}");
                continue;
            }
        };
        assert!(output.status.success(), "{shell} failed: {output:?}");
        assert_eq!(
            output.stdout, expected,
            "{shell} decoded {line} differently"
        );
        assert!(!std::path::Path::new("/tmp/sentinal-pwned").exists());
        eprintln!("{shell}: ok");
    }
}
