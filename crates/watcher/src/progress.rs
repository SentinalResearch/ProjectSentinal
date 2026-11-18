//! Fallback progress for programs no built-in adapter recognises.
//!
//! Only explicit counters count: a tqdm bar, a leading `[i/n]`, or a known
//! unit word followed by `i/n` or `i of n`. A bare fraction ("accuracy
//! 41/50", a date) is never progress, because nothing says it is a count.
//!
//! Programs can also print one explicit API line that is parsed first, ahead
//! of adapters and the fallback: `::sentinal progress current=3 total=50
//! unit=task` or `::sentinal metric name=loss value=0.81`.

/// Words that name what a counter counts.
const UNITS: &[&str] = &[
    "task",
    "sample",
    "example",
    "item",
    "step",
    "epoch",
    "iter",
    "iteration",
    "batch",
    "file",
    "case",
    "problem",
    "question",
    "record",
    "row",
    "page",
    "chunk",
    "request",
    "prompt",
    "test",
];

/// Maximum length of a metric name in a `::sentinal metric` line.
const MAX_METRIC_NAME_CHARS: usize = 64;
/// Maximum length of a progress unit in a `::sentinal progress` line.
const MAX_UNIT_CHARS: usize = 32;

/// A name or unit: letters, digits, `_`, `-` and `.`, nothing a terminal or a
/// page could misread.
fn safe_word(word: &str, max_chars: usize) -> bool {
    !word.is_empty()
        && word.chars().count() <= max_chars
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// What a `::sentinal` API line asks the watcher to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExplicitLine {
    /// `::sentinal progress current=N total=M [unit=U]`
    Progress {
        current: u64,
        total: Option<u64>,
        unit: String,
    },
    /// `::sentinal metric name=X value=Y`
    Metric { name: String, value: f64 },
    /// `::sentinal task name=… [step=N steps=M] [eta=12m]` or
    /// `::sentinal state value=working|blocked|waiting|done` (6.5).
    Agent(AgentLine),
}

/// What a script says about its own task through a `::sentinal task` or
/// `state` line (6.5). Applied like `sentinal agent-state`: shown, never
/// stored as an Event, and never a change of lifecycle state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentLine {
    /// From a `state` line; a `task` line leaves the status as it was.
    pub status: Option<protocol::AgentStatus>,
    /// From `name=`, at most 80 characters; may be double-quoted.
    pub task: Option<String>,
    /// `step=`, at most `steps`.
    pub step: Option<u32>,
    /// `steps=`, at most 10 000.
    pub steps: Option<u32>,
    /// `eta=` (`90s`, `12m`, `2h`), at most 7 days.
    pub eta_secs: Option<u32>,
}

/// Parse one explicit `::sentinal` API line, or `None` for anything else.
///
/// Malformed lines return `None` and are shown as normal output. The line is
/// untrusted output: it may only set progress and metrics.
pub(crate) fn explicit_line(line: &str) -> Option<ExplicitLine> {
    let rest = line.trim_start().strip_prefix("::sentinal ")?;
    let mut parts = rest.split_whitespace();
    match parts.next()? {
        "progress" => parse_progress(parts),
        "metric" => parse_metric(parts),
        "state" => parse_state(parts),
        "task" => parse_task(rest.trim_start().strip_prefix("task")?),
        _ => None,
    }
}

/// `value=working|blocked|waiting|done`.
fn parse_state<'a, I: Iterator<Item = &'a str>>(parts: I) -> Option<ExplicitLine> {
    let mut status = None;
    for part in parts {
        if let Some(value) = part.strip_prefix("value=") {
            status = Some(match value {
                "working" => protocol::AgentStatus::Working,
                "blocked" => protocol::AgentStatus::Blocked,
                "waiting" => protocol::AgentStatus::Waiting,
                "done" => protocol::AgentStatus::Done,
                _ => return None,
            });
        }
    }
    Some(ExplicitLine::Agent(AgentLine {
        status: Some(status?),
        ..AgentLine::default()
    }))
}

/// `name=migrate-tests step=3 steps=8 eta=12m`; `name="two words"` is allowed.
fn parse_task(rest: &str) -> Option<ExplicitLine> {
    let mut line = AgentLine::default();
    for (key, value) in fields(rest)? {
        match key {
            "name" => line.task = Some(value),
            "step" => line.step = Some(value.parse().ok()?),
            "steps" => line.steps = Some(value.parse().ok()?),
            "eta" => line.eta_secs = Some(protocol::parse_agent_eta(&value)?),
            _ => {}
        }
    }
    line.task.as_ref()?;
    protocol::validate_agent_progress(line.task.as_deref(), line.step, line.steps, line.eta_secs)
        .ok()?;
    Some(ExplicitLine::Agent(line))
}

/// Split `key=value key="quoted value"` pairs; `None` for an unclosed quote.
fn fields(text: &str) -> Option<Vec<(&str, String)>> {
    let mut out = Vec::new();
    let mut rest = text.trim_start();
    while !rest.is_empty() {
        let (key, after) = rest.split_once('=')?;
        if key.contains(char::is_whitespace) {
            // A bare word without `=`: skip it like other API lines do.
            let skip = key.find(char::is_whitespace).unwrap_or(key.len());
            rest = rest[skip..].trim_start();
            continue;
        }
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"')?;
            (quoted[..end].to_string(), &quoted[end + 1..])
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (after[..end].to_string(), &after[end..])
        };
        out.push((key, value));
        rest = next.trim_start();
    }
    Some(out)
}

/// `current=3 total=50 unit=task` (total and unit optional).
fn parse_progress<'a, I: Iterator<Item = &'a str>>(parts: I) -> Option<ExplicitLine> {
    let mut current = None;
    let mut total = None;
    let mut unit = None;
    // A field that is present but malformed rejects the whole line.
    for part in parts {
        if let Some(value) = part.strip_prefix("current=") {
            current = Some(value.parse::<u64>().ok()?);
        } else if let Some(value) = part.strip_prefix("total=") {
            total = Some(value.parse::<u64>().ok()?);
        } else if let Some(value) = part.strip_prefix("unit=") {
            if !safe_word(value, MAX_UNIT_CHARS) {
                return None;
            }
            unit = Some(value.to_string());
        }
    }
    let current = current?;
    if let Some(total) = total {
        if current > total {
            return None;
        }
    }
    Some(ExplicitLine::Progress {
        current,
        total,
        unit: unit.unwrap_or_else(|| "item".to_string()),
    })
}

/// `name=loss value=0.81`.
fn parse_metric<'a, I: Iterator<Item = &'a str>>(parts: I) -> Option<ExplicitLine> {
    let mut name = None;
    let mut value = None;
    for part in parts {
        if let Some(v) = part.strip_prefix("name=") {
            name = Some(v.to_string());
        } else if let Some(v) = part.strip_prefix("value=") {
            value = Some(v.parse::<f64>().ok()?);
        }
    }
    let name = name?;
    let value = value?;
    if !value.is_finite() || !safe_word(&name, MAX_METRIC_NAME_CHARS) {
        return None;
    }
    Some(ExplicitLine::Metric { name, value })
}

/// `(current, total, unit)` when `line` reports explicit progress.
pub(crate) fn fallback_progress(line: &str) -> Option<(u64, u64, String)> {
    let found = tqdm(line)
        .map(|(current, total)| (current, total, "item".to_string()))
        .or_else(|| bracketed(line).map(|(current, total)| (current, total, "item".to_string())))
        .or_else(|| labelled(line))?;
    (found.1 > 0 && found.0 <= found.1).then_some(found)
}

/// tqdm-style `label:  42%|████     | 21/50 [...]`.
fn tqdm(line: &str) -> Option<(u64, u64)> {
    let after = &line[line.find("%|")? + 2..];
    let rest = after[after.find('|')? + 1..].trim_start();
    fraction(rest.split_whitespace().next()?)
}

/// A line starting with `[i/n]`, as ninja and many scripts print.
fn bracketed(line: &str) -> Option<(u64, u64)> {
    let inner = line.trim_start().strip_prefix('[')?;
    let (inner, _) = inner.split_once(']')?;
    fraction(inner.trim())
}

/// `task 3/50`, `Processing sample 12 of 200`, `Step: 5/20`.
fn labelled(line: &str) -> Option<(u64, u64, String)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        let unit = word
            .trim_start_matches(['(', '['])
            .trim_end_matches([':', '#'])
            .to_ascii_lowercase();
        if !UNITS.contains(&unit.as_str()) {
            continue;
        }
        let Some(next) = words.get(index + 1).map(|w| w.trim_start_matches('#')) else {
            continue;
        };
        if let Some(found) = fraction(next) {
            return Some((found.0, found.1, unit));
        }
        let of = words
            .get(index + 2)
            .is_some_and(|w| w.eq_ignore_ascii_case("of"));
        if let (true, Some(current), Some(total)) = (
            of,
            number(next),
            words.get(index + 3).and_then(|w| number(w)),
        ) {
            return Some((current, total, unit));
        }
    }
    None
}

/// `21/50` (trailing punctuation allowed, nothing else).
fn fraction(token: &str) -> Option<(u64, u64)> {
    let (current, total) = token.split_once('/')?;
    Some((current.parse().ok()?, number(total)?))
}

/// A whole number with trailing punctuation (`50,` `50:` `50)`) removed.
fn number(token: &str) -> Option<u64> {
    let digits = token.trim_end_matches([',', ':', ';', ')', ']', '.']);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{explicit_line, fallback_progress, ExplicitLine};

    #[test]
    fn counters_are_found_and_bare_fractions_are_not() {
        assert_eq!(
            fallback_progress("Task: 3/50,"),
            Some((3, 50, "task".into()))
        );
        assert_eq!(
            fallback_progress("question #4 of 9"),
            Some((4, 9, "question".into()))
        );
        assert_eq!(
            fallback_progress("[ 2/8 ] done"),
            Some((2, 8, "item".into()))
        );
        assert_eq!(fallback_progress("41/50 correct"), None);
        assert_eq!(fallback_progress("task 2026/11/03"), None);
        assert_eq!(fallback_progress(""), None);
    }

    #[test]
    fn explicit_progress_line_is_parsed() {
        assert_eq!(
            explicit_line("::sentinal progress current=3 total=50 unit=task"),
            Some(ExplicitLine::Progress {
                current: 3,
                total: Some(50),
                unit: "task".into(),
            })
        );
        assert_eq!(
            explicit_line("::sentinal progress current=10"),
            Some(ExplicitLine::Progress {
                current: 10,
                total: None,
                unit: "item".into(),
            })
        );
    }

    #[test]
    fn explicit_metric_line_is_parsed() {
        assert_eq!(
            explicit_line("::sentinal metric name=loss value=0.81"),
            Some(ExplicitLine::Metric {
                name: "loss".into(),
                value: 0.81,
            })
        );
    }

    #[test]
    fn malformed_explicit_lines_are_ignored() {
        assert_eq!(explicit_line("::sentinal progress current=5 total=3"), None);
        assert_eq!(explicit_line("::sentinal progress"), None);
        assert_eq!(explicit_line("::sentinal metric name=loss value=NaN"), None);
        assert_eq!(explicit_line("::sentinal metric name= value=1.0"), None);
        assert_eq!(explicit_line("::sentinal unknown foo=bar"), None);
        assert_eq!(explicit_line("::sentinal"), None);
        assert_eq!(explicit_line("normal output"), None);
        assert_eq!(
            explicit_line(&format!(
                "::sentinal metric name={} value=1.0",
                "x".repeat(65)
            )),
            None
        );
    }
}
