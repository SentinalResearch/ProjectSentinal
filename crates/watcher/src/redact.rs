//! Hide secret-shaped values in terminal text before it is shown elsewhere
//! (Milestone 5.S output search). Deterministic and conservative: values
//! named like secrets, credentials in URLs, bearer tokens, and well-known
//! token formats. Ordinary output (hashes, paths, numbers) is left alone.

use regex::Regex;
use std::sync::OnceLock;

const REDACTED: &str = "[redacted]";

fn patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // A private key header hides the whole line.
            (r"^.*-----BEGIN [A-Z ]*PRIVATE KEY-----.*$", REDACTED),
            // name=value / name: value where the name says secret.
            (
                r#"(?i)\b([A-Z0-9_.-]*(?:password|passwd|secret|token|api[_-]?key|access[_-]?key|private[_-]?key|auth[_-]?key|credential)[A-Z0-9_.-]*)(\s*[=:]\s*)("[^"]*"|'[^']*'|\S+)"#,
                "$1$2[redacted]",
            ),
            // Authorization: Bearer xyz
            (r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{8,}", "${1}[redacted]"),
            // scheme://user:pass@host
            (r"://[^/\s:@]+:[^/\s@]+@", "://[redacted]@"),
            // Well-known token formats.
            (
                r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{8,}|sk-[A-Za-z0-9_-]{16,}|xox[abpr]-[A-Za-z0-9-]{8,}|AKIA[0-9A-Z]{16}|tskey-[A-Za-z0-9-]{8,}|sat_agt_[0-9a-f]{16}_[0-9a-f]{16,})",
                REDACTED,
            ),
        ]
        .into_iter()
        .filter_map(|(pattern, replacement)| {
            Regex::new(pattern).ok().map(|regex| (regex, replacement))
        })
        .collect()
    })
}

/// `line` with secret-shaped values replaced by `[redacted]`.
pub fn redact_secrets(line: &str) -> String {
    patterns()
        .iter()
        .fold(line.to_string(), |text, (regex, replacement)| {
            regex.replace_all(&text, *replacement).into_owned()
        })
}
