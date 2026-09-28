//! Installation and shell-specific loading of embedded OSC hook scripts.

use std::{
    fs,
    path::{Path, PathBuf},
};

const ZSH_HOOK: &str = include_str!("../../../shell/ta.zsh");
const BASH_HOOK: &str = include_str!("../../../shell/ta.bash");
const BASH_PREEXEC: &str = include_str!("../../../shell/vendor/bash-preexec.sh");
const FISH_HOOK: &str = include_str!("../../../shell/ta.fish");

/// Arguments and environment needed to load hooks for one shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellLaunch {
    /// Extra shell arguments.
    pub args: Vec<String>,
    /// Extra environment entries.
    pub env: Vec<(String, String)>,
}

/// Errors installing embedded shell hook files.
#[derive(Debug, thiserror::Error)]
pub enum ShellHookError {
    /// A hook directory or file could not be written.
    #[error("failed to install shell hook '{}': {source}", path.display())]
    File {
        /// Path that could not be written.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
}

/// Install embedded hooks and wrapper startup files under the daemon data directory.
pub fn install_hooks(data_dir: &Path, home: &Path) -> Result<PathBuf, ShellHookError> {
    let root = data_dir.join("shell");
    write(&root.join("ta.zsh"), ZSH_HOOK)?;
    write(&root.join("ta.bash"), BASH_HOOK)?;
    write(&root.join("ta.fish"), FISH_HOOK)?;
    write(&root.join("vendor/bash-preexec.sh"), BASH_PREEXEC)?;

    let quoted_root = shell_quote(&root.to_string_lossy());
    let quoted_home = shell_quote(&home.to_string_lossy());
    write(
        &root.join("zsh/.zshrc"),
        &format!(
            "export TA_HOOK_DIR={quoted_root}\nsource {quoted_root}/ta.zsh\nif [[ -n \"${{TA_USER_ZDOTDIR:-}}\" && -f \"$TA_USER_ZDOTDIR/.zshrc\" ]]; then source \"$TA_USER_ZDOTDIR/.zshrc\"; elif [[ -f {quoted_home}/.zshrc ]]; then source {quoted_home}/.zshrc; fi\n"
        ),
    )?;
    write(
        &root.join("bashrc"),
        &format!(
            "export TA_HOOK_DIR={quoted_root}\nsource {quoted_root}/ta.bash\n[[ -f {quoted_home}/.bashrc ]] && source {quoted_home}/.bashrc\n"
        ),
    )?;
    Ok(root)
}

/// Select startup arguments and environment for zsh, bash, or fish.
pub fn launch(shell: &Path, hook_dir: &Path) -> ShellLaunch {
    let name = shell
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match name {
        "zsh" => ShellLaunch {
            args: Vec::new(),
            env: {
                let mut env = vec![(
                    "ZDOTDIR".into(),
                    hook_dir.join("zsh").to_string_lossy().into_owned(),
                )];
                if let Some(user_zdotdir) = std::env::var_os("ZDOTDIR") {
                    env.push((
                        "TA_USER_ZDOTDIR".into(),
                        user_zdotdir.to_string_lossy().into_owned(),
                    ));
                }
                env
            },
        },
        "bash" => ShellLaunch {
            args: vec![
                "--rcfile".into(),
                hook_dir.join("bashrc").to_string_lossy().into_owned(),
            ],
            env: vec![(
                "TA_HOOK_DIR".into(),
                hook_dir.to_string_lossy().into_owned(),
            )],
        },
        "fish" => ShellLaunch {
            args: vec![
                "-C".into(),
                format!(
                    "source {}",
                    fish_quote(&hook_dir.join("ta.fish").to_string_lossy())
                ),
            ],
            env: Vec::new(),
        },
        _ => ShellLaunch::default(),
    }
}

fn write(path: &Path, contents: &str) -> Result<(), ShellHookError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ShellHookError::File {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, contents).map_err(|source| ShellHookError::File {
        path: path.to_path_buf(),
        source,
    })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn fish_quote(value: &str) -> String {
    shell_quote(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_launches_use_the_required_loading_mechanism() {
        let hooks = Path::new("/tmp/terminal agent/hooks");
        let zsh = launch(Path::new("/bin/zsh"), hooks);
        assert!(zsh.env.iter().any(|(key, _)| key == "ZDOTDIR"));
        let bash = launch(Path::new("/bin/bash"), hooks);
        assert_eq!(bash.args.first().map(String::as_str), Some("--rcfile"));
        let fish = launch(Path::new("/usr/bin/fish"), hooks);
        assert_eq!(fish.args.first().map(String::as_str), Some("-C"));
    }

    #[test]
    fn unknown_shell_still_launches_without_hooks() {
        assert_eq!(
            launch(Path::new("/bin/sh"), Path::new("/tmp/hooks")),
            ShellLaunch::default()
        );
    }
}
