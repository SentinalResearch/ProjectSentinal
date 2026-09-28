//! rc selection, safe on-disk edits, and generated hook scripts.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::{
    hook_script::{ensure_not_world_writable, loader_body, render_hook, resolve_binary, HookShell},
    rc_file,
    rc_install::{apply, apply_with_rename, select_rc, Applied, InstallError, RcEnv},
};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-rc-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

fn env(home: &Path, macos: bool) -> RcEnv {
    RcEnv {
        home: home.to_path_buf(),
        shell: Some("/bin/zsh".into()),
        zdotdir: None,
        xdg_config_home: None,
        macos,
    }
}

const BODY: &str = "if [ -x '/bin/sentinal' ]; then eval \"$('/bin/sentinal' hook zsh)\"; fi\n";

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

fn backups(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().contains(".sentinal-backup-"))
        .collect();
    found.sort();
    found
}

// ---- selection -------------------------------------------------------------

#[test]
fn shell_comes_from_the_argument_or_shell_env_and_never_guesses() {
    let home = temp();
    let (shell, path) = select_rc(None, None, &env(&home, false)).unwrap();
    assert_eq!((shell, path), (HookShell::Zsh, home.join(".zshrc")));

    let mut unknown = env(&home, false);
    unknown.shell = Some("/bin/tcsh".into());
    assert!(matches!(
        select_rc(None, None, &unknown),
        Err(InstallError::UnknownShell(_))
    ));
    let (shell, _) = select_rc(Some(HookShell::Fish), None, &unknown).unwrap();
    assert_eq!(shell, HookShell::Fish);
}

#[test]
fn rc_paths_follow_each_shells_conventions() {
    let home = temp();
    let mut rc_env = env(&home, false);
    rc_env.zdotdir = Some(home.join("zdot"));
    rc_env.xdg_config_home = Some(home.join("xdg"));
    let pick = |shell| select_rc(Some(shell), None, &rc_env).unwrap().1;
    assert_eq!(pick(HookShell::Zsh), home.join("zdot/.zshrc"));
    assert_eq!(pick(HookShell::Fish), home.join("xdg/fish/config.fish"));
    assert_eq!(pick(HookShell::Bash), home.join(".bashrc"));
    let explicit = select_rc(Some(HookShell::Bash), Some(home.join("my.rc")), &rc_env).unwrap();
    assert_eq!(explicit.1, home.join("my.rc"));
}

#[test]
fn macos_bash_uses_the_login_file_bash_actually_reads() {
    let home = temp();
    let macos = env(&home, true);
    let pick = || select_rc(Some(HookShell::Bash), None, &macos);
    assert_eq!(pick().unwrap().1, home.join(".bash_profile"));

    fs::write(home.join(".profile"), "PATH=/x\n").unwrap();
    let error = pick().unwrap_err();
    assert!(error.to_string().contains("--rc"), "{error}");

    fs::write(home.join(".bash_login"), "").unwrap();
    assert_eq!(pick().unwrap().1, home.join(".bash_login"));
    fs::write(home.join(".bash_profile"), "").unwrap();
    assert_eq!(pick().unwrap().1, home.join(".bash_profile"));
}

// ---- applying --------------------------------------------------------------

#[test]
fn install_creates_a_missing_file_and_parent_without_a_backup() {
    let home = temp();
    let rc = home.join(".config/fish/config.fish");
    let applied = apply(&rc, 1, |original| rc_file::install(original, BODY)).unwrap();
    assert_eq!(applied, Applied::Written { backup: None });
    assert!(rc_file::is_installed(&fs::read(&rc).unwrap()).unwrap());
    assert_eq!(mode(&rc) & 0o022, 0, "new rc is not group/world-writable");
}

#[test]
fn install_backs_up_first_preserves_mode_and_is_then_idempotent() {
    let home = temp();
    let rc = home.join(".zshrc");
    fs::write(&rc, "alias g=git").unwrap();
    fs::set_permissions(&rc, fs::Permissions::from_mode(0o640)).unwrap();

    let applied = apply(&rc, 1_790_000_000_000, |o| rc_file::install(o, BODY)).unwrap();
    let Applied::Written {
        backup: Some(backup),
    } = applied
    else {
        panic!("expected a backup");
    };
    assert_eq!(fs::read(&backup).unwrap(), b"alias g=git");
    assert_eq!(mode(&backup), 0o600);
    assert!(backup
        .to_string_lossy()
        .ends_with(".sentinal-backup-1790000000000"));
    assert_eq!(mode(&rc), 0o640);

    let before = fs::read(&rc).unwrap();
    let again = apply(&rc, 1_790_000_000_001, |o| rc_file::install(o, BODY)).unwrap();
    assert_eq!(again, Applied::Unchanged);
    assert_eq!(fs::read(&rc).unwrap(), before);
    assert_eq!(
        backups(&home).len(),
        1,
        "an unchanged install makes no backup"
    );

    apply(&rc, 1_790_000_000_002, rc_file::uninstall).unwrap();
    assert_eq!(fs::read(&rc).unwrap(), b"alias g=git");
    assert_eq!(mode(&rc), 0o640);
    assert_eq!(backups(&home).len(), 2);
}

#[test]
fn malformed_blocks_leave_the_file_untouched() {
    let home = temp();
    let rc = home.join(".bashrc");
    let text = format!("{}\nhalf a block\n", rc_file::BEGIN);
    fs::write(&rc, &text).unwrap();
    let error = apply(&rc, 1, |o| rc_file::install(o, BODY)).unwrap_err();
    assert!(error.to_string().contains("never ends"), "{error}");
    assert_eq!(fs::read_to_string(&rc).unwrap(), text);
    assert!(backups(&home).is_empty());
}

#[test]
fn a_failed_atomic_replace_keeps_the_original_and_leaves_no_temp_file() {
    let home = temp();
    let rc = home.join(".zshrc");
    fs::write(&rc, "keep me\n").unwrap();
    let error = apply_with_rename(
        &rc,
        1,
        |o| rc_file::install(o, BODY),
        |_, _| Err(std::io::Error::other("disk full")),
    )
    .unwrap_err();
    assert!(error.to_string().contains("disk full"), "{error}");
    assert_eq!(fs::read_to_string(&rc).unwrap(), "keep me\n");
    let leftovers: Vec<_> = fs::read_dir(&home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("sentinal-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_symlinked_rc_is_edited_at_its_target_and_the_link_survives() {
    let home = temp();
    let dotfiles = home.join("dotfiles");
    fs::create_dir_all(&dotfiles).unwrap();
    fs::write(dotfiles.join("zshrc"), "x\n").unwrap();
    let rc = home.join(".zshrc");
    std::os::unix::fs::symlink(dotfiles.join("zshrc"), &rc).unwrap();

    apply(&rc, 1, |o| rc_file::install(o, BODY)).unwrap();
    assert!(fs::symlink_metadata(&rc).unwrap().file_type().is_symlink());
    assert!(rc_file::is_installed(&fs::read(dotfiles.join("zshrc")).unwrap()).unwrap());
}

#[test]
fn world_writable_rc_files_and_directories_are_refused() {
    let home = temp();
    let rc = home.join(".zshrc");
    fs::write(&rc, "x\n").unwrap();
    fs::set_permissions(&rc, fs::Permissions::from_mode(0o666)).unwrap();
    let error = apply(&rc, 1, |o| rc_file::install(o, BODY)).unwrap_err();
    assert!(error.to_string().contains("world-writable"), "{error}");
    assert_eq!(fs::read_to_string(&rc).unwrap(), "x\n");

    let open_dir = temp();
    fs::set_permissions(&open_dir, fs::Permissions::from_mode(0o777)).unwrap();
    let error = apply(&open_dir.join(".zshrc"), 1, |o| rc_file::install(o, BODY)).unwrap_err();
    assert!(error.to_string().contains("world-writable"), "{error}");
    assert!(!open_dir.join(".zshrc").exists());
}

// ---- scripts and binary ----------------------------------------------------

#[test]
fn hooks_embed_the_quoted_binary_and_honor_suppression_variables() {
    let binary = Path::new("/opt/my tools/sentinal");
    for shell in [HookShell::Zsh, HookShell::Bash, HookShell::Fish] {
        let script = render_hook(shell, binary).unwrap();
        assert!(
            script.contains("'/opt/my tools/sentinal' hook report"),
            "{shell:?}"
        );
        assert!(!script.contains("@SENTINAL_BIN@"));
        assert!(script.contains("SENTINAL_SESSION_ID"));
        assert!(script.contains("SENTINAL_DISABLE"));
        assert!(script.contains("SENTINAL_HOOK_TEXT"));
        let loader = loader_body(shell, binary).unwrap();
        assert!(
            loader.contains(&format!("hook {}", shell.name())),
            "{loader}"
        );
        assert!(loader.contains("2>/dev/null"));
    }
}

#[test]
fn the_path_entry_is_preferred_when_it_is_the_same_binary() {
    let dir = temp();
    let real = dir.join("real-sentinal");
    fs::write(&real, "").unwrap();
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&real, bin.join("sentinal")).unwrap();
    let path: OsString = std::env::join_paths([Path::new("/nonexistent"), &bin]).unwrap();

    assert_eq!(resolve_binary(&real, Some(&path)), bin.join("sentinal"));
    assert_eq!(resolve_binary(&real, None), real);
}

#[test]
fn a_binary_in_a_world_writable_directory_is_refused() {
    let dir = temp();
    let binary = dir.join("sentinal");
    fs::write(&binary, "").unwrap();
    assert!(ensure_not_world_writable(&binary).is_ok());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(ensure_not_world_writable(&binary).is_err());
}
