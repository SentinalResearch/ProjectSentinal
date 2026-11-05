//! Fallback progress for programs no built-in adapter recognises.
//!
//! Only explicit counters count: a tqdm bar, a leading `[i/n]`, or a known
//! unit word followed by `i/n` or `i of n`. A bare fraction ("accuracy
//! 41/50", a date) is never progress, because nothing says it is a count.

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
    use super::fallback_progress;

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
}
