//! Search everywhere (Milestone 5.S): the request, its hits, and the ranking.
//!
//! Each Machine searches its own store and ranks its hits with
//! [`match_score`] and [`rank_score`]; the browser merges every Machine's hits
//! by score and ranks what it already holds (Machines, Settings pages) with
//! the same formula, ported to `web/src/data/search.ts`. Both are tested
//! against `tests/fixtures/search_ranking.json`.

use crate::{CommandId, EventId, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// Longest query accepted, in UTF-8 bytes.
pub const MAX_SEARCH_QUERY_BYTES: usize = 256;
/// Most hits one Machine returns for one search.
pub const MAX_SEARCH_LIMIT: u32 = 50;
/// Longest hit title, in characters.
pub const MAX_HIT_TITLE_CHARS: usize = 160;
/// Longest hit detail, in characters.
pub const MAX_HIT_DETAIL_CHARS: usize = 200;

/// Match tiers: the query at the start, at a word start, anywhere, or as a subsequence.
pub const MATCH_PREFIX: f64 = 100.0;
/// See [`MATCH_PREFIX`].
pub const MATCH_WORD_START: f64 = 80.0;
/// See [`MATCH_PREFIX`].
pub const MATCH_SUBSTRING: f64 = 60.0;
/// Every character at a word start, in order: `oom` in "out of memory".
pub const MATCH_INITIALS: f64 = 40.0;
/// See [`MATCH_PREFIX`].
pub const MATCH_FUZZY: f64 = 20.0;
/// Bonus for something that just happened, halving every [`RECENCY_HALF_LIFE_MS`].
pub const RECENCY_BOOST: f64 = 15.0;
/// See [`RECENCY_BOOST`].
pub const RECENCY_HALF_LIFE_MS: f64 = 86_400_000.0;
/// Bonus for a Session waiting on the owner.
pub const NEEDS_YOU_BOOST: f64 = 30.0;

/// What a search looks through, and what kind of thing a hit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchScope {
    /// Session titles and their running Commands.
    Sessions,
    /// Command history.
    Commands,
    /// Errors, warnings, prompts, checkpoints, and servers.
    Events,
    /// Recipes on the Machine.
    Recipes,
    /// Saved terminal output of Sessions the owner opted in (FULL capture
    /// only); scanned on demand, never indexed, secrets redacted.
    Output,
}

impl SearchScope {
    /// Every scope a Machine searches.
    pub const ALL: [SearchScope; 5] = [
        Self::Sessions,
        Self::Commands,
        Self::Events,
        Self::Recipes,
        Self::Output,
    ];
}

/// How a hit ended, for `find failed …`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchOutcome {
    /// Non-zero exit, a failed Session, or an error Event.
    Failed,
    /// Exit code 0 or a completed Session.
    Succeeded,
    /// Still running.
    Running,
}

/// Optional narrowing; the browser applies `on <machine>` by asking only that Machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SearchFilters {
    /// Only hits at or after this time.
    #[serde(default)]
    pub since: Option<Timestamp>,
    /// Only hits with this outcome. Recipes have none and are left out.
    #[serde(default)]
    pub outcome: Option<SearchOutcome>,
}

impl SearchFilters {
    /// True when nothing is filtered.
    pub fn is_empty(&self) -> bool {
        self.since.is_none() && self.outcome.is_none()
    }
}

/// Where a hit leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchTarget {
    /// A Session page.
    Session {
        /// The Session.
        session_id: SessionId,
    },
    /// A Command in its Session.
    Command {
        /// The Session it ran in.
        session_id: SessionId,
        /// The Command.
        command_id: CommandId,
    },
    /// An Event in its Session.
    Event {
        /// The Session it happened in.
        session_id: SessionId,
        /// The Event.
        event_id: EventId,
    },
    /// A Recipe.
    Recipe {
        /// The Recipe.
        recipe_id: String,
    },
}

/// One ranked result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SearchHit {
    /// What kind of thing this is.
    pub kind: SearchScope,
    /// One line, at most [`MAX_HIT_TITLE_CHARS`].
    pub title: String,
    /// One line of context, at most [`MAX_HIT_DETAIL_CHARS`].
    pub detail: String,
    /// Where it leads.
    pub target: SearchTarget,
    /// When it happened, if it has a time.
    pub at: Option<Timestamp>,
    /// [`rank_score`]; higher is better.
    pub score: f64,
}

/// How well `query` matches any of `haystacks`: 0 when some word matches
/// none, else the mean over the query's words of the best tier each reaches
/// (prefix > word start > substring > initials > subsequence).
/// An empty query scores 1. Case is ignored.
pub fn match_score(query: &str, haystacks: &[&str]) -> f64 {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return 1.0;
    }
    let hays: Vec<String> = haystacks.iter().map(|hay| hay.to_lowercase()).collect();
    let mut total = 0.0;
    for word in &words {
        let best = hays
            .iter()
            .map(|hay| word_score(word, hay))
            .fold(0.0, f64::max);
        if best == 0.0 {
            return 0.0;
        }
        total += best;
    }
    total / words.len() as f64
}

fn word_score(word: &str, hay: &str) -> f64 {
    match hay.find(word) {
        Some(0) => MATCH_PREFIX,
        Some(at) => {
            let before = hay[..at].chars().next_back();
            if before.is_some_and(|c| !is_word_char(c)) {
                MATCH_WORD_START
            } else {
                MATCH_SUBSTRING
            }
        }
        None if is_initials(word, hay) => MATCH_INITIALS,
        None if is_subsequence(word, hay) => MATCH_FUZZY,
        None => 0.0,
    }
}

fn is_word_char(c: char) -> bool {
    matches!(c, 'a'..='z' | '0'..='9')
}

/// Each character of `needle` found, in order, at the start of a word of `hay`.
fn is_initials(needle: &str, hay: &str) -> bool {
    let mut wanted = needle.chars().peekable();
    let mut previous: Option<char> = None;
    for c in hay.chars() {
        let starts_word = previous.is_none_or(|p| !is_word_char(p));
        if starts_word && wanted.peek() == Some(&c) {
            wanted.next();
        }
        if wanted.peek().is_none() {
            return true;
        }
        previous = Some(c);
    }
    false
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut wanted = needle.chars().peekable();
    for c in hay.chars() {
        if wanted.peek() == Some(&c) {
            wanted.next();
        }
        if wanted.peek().is_none() {
            return true;
        }
    }
    wanted.peek().is_none()
}

/// The final score: the match plus recency and needs-you boosts, rounded to
/// hundredths so the daemon and the browser agree exactly. A non-match stays 0.
pub fn rank_score(matched: f64, age_ms: i64, needs_you: bool) -> f64 {
    if matched <= 0.0 {
        return 0.0;
    }
    let age = age_ms.max(0) as f64;
    let recency = RECENCY_BOOST * 0.5_f64.powf(age / RECENCY_HALF_LIFE_MS);
    let bonus = if needs_you { NEEDS_YOU_BOOST } else { 0.0 };
    ((matched + recency + bonus) * 100.0).round() / 100.0
}

/// One display line: whitespace runs collapsed, at most `max` characters,
/// ending in `…` when cut.
pub fn clip(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}
