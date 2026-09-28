//! Byte-exact rc-file block fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::rc_file::{install, is_installed, uninstall, RcError, BEGIN, BEGIN_ADDED_NEWLINE, END};

const BODY: &str = "# Managed by `sentinal hook install`.\nif [ -x '/bin/sentinal' ]; then eval \"$('/bin/sentinal' hook zsh)\"; fi\n";

fn block(begin: &str) -> String {
    format!("{begin}\n{BODY}{END}\n")
}

#[test]
fn empty_or_missing_file_gets_only_the_block_and_uninstall_empties_it() {
    let installed = install(b"", BODY).unwrap();
    assert_eq!(installed, block(BEGIN).as_bytes());
    assert_eq!(uninstall(&installed).unwrap(), b"");
}

#[test]
fn existing_content_is_kept_byte_for_byte() {
    let original = b"export PATH=\"$HOME/bin:$PATH\"\nalias ll='ls -l'\n";
    let installed = install(original, BODY).unwrap();
    assert!(installed.starts_with(original));
    assert_eq!(&installed[original.len()..], block(BEGIN).as_bytes());
    assert_eq!(uninstall(&installed).unwrap(), original);
}

#[test]
fn a_file_without_a_final_newline_is_restored_without_one() {
    let original = b"source ~/.aliases";
    let installed = install(original, BODY).unwrap();
    assert_eq!(
        installed,
        format!("source ~/.aliases\n{}", block(BEGIN_ADDED_NEWLINE)).as_bytes()
    );
    assert_eq!(uninstall(&installed).unwrap(), original);
}

#[test]
fn a_second_install_is_byte_for_byte_idempotent() {
    let once = install(b"a\n", BODY).unwrap();
    let twice = install(&once, BODY).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn a_changed_loader_is_updated_in_place() {
    let once = install(b"a\n", BODY).unwrap();
    let moved = BODY.replace("/bin/sentinal", "/opt/bin/sentinal");
    let updated = install(&once, &moved).unwrap();
    assert_eq!(updated, format!("a\n{BEGIN}\n{moved}{END}\n").as_bytes());
    assert_eq!(uninstall(&updated).unwrap(), b"a\n");
}

#[test]
fn uninstall_keeps_unrelated_edits_made_after_install() {
    let installed = install(b"a\n", BODY).unwrap();
    let mut edited = b"# added before\n".to_vec();
    edited.extend_from_slice(&installed);
    edited.extend_from_slice(b"export EDITOR=vim\n");
    assert_eq!(
        uninstall(&edited).unwrap(),
        b"# added before\na\nexport EDITOR=vim\n"
    );
}

#[test]
fn non_utf8_bytes_are_preserved() {
    let original = b"# caf\xe9 latin-1 comment\n";
    let installed = install(original, BODY).unwrap();
    assert_eq!(uninstall(&installed).unwrap(), original);
}

#[test]
fn uninstall_without_a_block_changes_nothing() {
    assert_eq!(uninstall(b"plain\n").unwrap(), b"plain\n");
    assert!(!is_installed(b"plain\n").unwrap());
}

#[test]
fn crlf_files_are_refused() {
    assert_eq!(install(b"a\r\nb\r\n", BODY), Err(RcError::Crlf));
}

#[test]
fn malformed_markers_are_refused_with_a_line_number() {
    let cases: Vec<(String, &str)> = vec![
        (format!("{BEGIN}\nx\n"), "never ends"),
        (format!("x\n{END}\n"), "never began"),
        (format!("{END}\n{BEGIN}\n"), "never began"),
        (format!("{BEGIN}\n{BEGIN}\n{END}\n"), "second begin"),
        (format!("{}{}", block(BEGIN), block(BEGIN)), "second begin"),
        (format!("  {BEGIN}\n{END}\n"), "edited begin"),
        (format!("{BEGIN}\n{END} # mine\n"), "edited end"),
    ];
    for (text, expected) in cases {
        for result in [install(text.as_bytes(), BODY), uninstall(text.as_bytes())] {
            let Err(RcError::Malformed(reason)) = result else {
                panic!("{text:?} was accepted");
            };
            assert!(reason.contains(expected), "{text:?}: {reason}");
            assert!(reason.contains("line"), "{reason}");
        }
    }
}

#[test]
fn a_removed_recorded_newline_is_malformed_not_guessed() {
    let installed = install(b"x", BODY).unwrap();
    let damaged = installed.replacen(b"x\n", b"x", 1);
    assert!(matches!(uninstall(&damaged), Err(RcError::Malformed(_))));
}

trait ReplaceBytes {
    fn replacen(&self, from: &[u8], to: &[u8], count: usize) -> Vec<u8>;
}

impl ReplaceBytes for Vec<u8> {
    fn replacen(&self, from: &[u8], to: &[u8], _count: usize) -> Vec<u8> {
        let at = self
            .windows(from.len())
            .position(|window| window == from)
            .unwrap();
        let mut out = self[..at].to_vec();
        out.extend_from_slice(to);
        out.extend_from_slice(&self[at + from.len()..]);
        out
    }
}
