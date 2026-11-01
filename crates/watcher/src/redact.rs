//! Remove secrets from terminal text before any agent or model sees it (6.2).
//!
//! The same patterns as `intelligence/sentinal_intelligence/sanitize.py`:
//! known token shapes are replaced whole; values after `Bearer`, URL
//! passwords, secret-named URL query parameters, `netrc file:`, and
//! `NAME=value` where NAME says token/secret/password/key keep their prefix.

use crate::LineFramer;
use regex::Regex;
use std::sync::OnceLock;

/// What a removed value becomes.
pub const REDACTED: &str = "[redacted]";

struct Patterns {
    whole: Vec<Regex>,
    context: Vec<Regex>,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let compile = |list: &[&str]| {
            list.iter()
                .filter_map(|pattern| Regex::new(pattern).ok())
                .collect::<Vec<_>>()
        };
        Patterns {
            whole: compile(&[
                r"\bhf_[A-Za-z0-9]{20,}",
                r"\bsk-[A-Za-z0-9_-]{20,}",
                r"\bAKIA[A-Z0-9]{16}\b",
                r"\bgh[pousr]_[A-Za-z0-9]{20,}",
                r"\bglpat-[A-Za-z0-9_-]{20,}",
                r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
            ]),
            context: compile(&[
                r"(?i)(\bBearer\s+)([A-Za-z0-9._~+/=-]{8,})",
                r"(://[^/\s:@]+:)([^/\s@]+)(@)",
                r"(?i)([?&](?:token|key|api_key|apikey|access_token|auth|sig|signature|password|secret)=)([^&\s#]+)",
                r"(?i)(\bnetrc file:\s*)(\S+)",
                r#"(?i)(\b[A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_?KEY|ACCESS_KEY)[A-Z0-9_]*\s*[=:]\s*)([^\s'"]+)"#,
            ]),
        }
    })
}

/// `text` with secrets replaced, and how many were replaced.
pub fn redact(text: &str) -> (String, u32) {
    let mut count = 0_u32;
    let mut text = text.to_string();
    for pattern in &patterns().whole {
        text = pattern
            .replace_all(&text, |_: &regex::Captures<'_>| {
                count += 1;
                REDACTED
            })
            .into_owned();
    }
    for pattern in &patterns().context {
        text = pattern
            .replace_all(&text, |captures: &regex::Captures<'_>| {
                let value = &captures[2];
                let suffix = captures.get(3).map_or("", |m| m.as_str());
                if value.contains(REDACTED) {
                    return format!("{}{value}{suffix}", &captures[1]);
                }
                count += 1;
                format!("{}{REDACTED}{suffix}", &captures[1])
            })
            .into_owned();
    }
    (text, count)
}

/// The last `max_lines` non-empty screen lines: escapes stripped, carriage
/// return redraws resolved to what was drawn last, secrets redacted.
pub fn screen_text(raw_lines: &[String], max_lines: usize) -> (Vec<String>, u32) {
    let mut redacted = 0;
    let mut lines: Vec<String> = Vec::new();
    for raw in raw_lines.iter().rev() {
        if lines.len() >= max_lines {
            break;
        }
        let mut framer = LineFramer::default();
        let mut parts = framer.push(raw.as_bytes());
        parts.extend(framer.finish());
        let Some(drawn) = parts.into_iter().rev().find(|part| !part.trim().is_empty()) else {
            continue;
        };
        let (clean, count) = redact(drawn.trim_end());
        redacted += count;
        lines.push(clean);
    }
    lines.reverse();
    (lines, redacted)
}
