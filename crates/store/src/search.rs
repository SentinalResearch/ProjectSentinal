//! Search candidates (Milestone 5.S).
//!
//! Indexed rows come from the trigram indexes (words of three or more
//! characters, anywhere in the text) or the unicode61 prefix index (shorter
//! words). The most recent rows are added so fuzzy matches the indexes cannot
//! express ("oom" for "out of memory") are still found. Every candidate is
//! kept only if [`match_score`] accepts it, so callers get real matches.

use crate::{records, Store, StoreError};
use protocol::{search::match_score, Command, Event, EventKind, Timestamp};
use rusqlite::{params, Connection};

/// Recent Commands and text-bearing Events matched without an index.
pub const RECENT_SEARCH_WINDOW: usize = 1_000;
/// Most indexed rows read per scope, newest first.
const INDEXED_CANDIDATES: u32 = 500;
/// Most recent Event rows scanned to find the recent text-bearing ones.
const RECENT_EVENT_SCAN: u32 = 20_000;
/// Words shorter than this use the prefix index instead of the trigram index.
const TRIGRAM: usize = 3;

/// What to look for.
#[derive(Debug, Clone, Copy)]
pub struct SearchCandidateQuery<'a> {
    /// The owner's text.
    pub text: &'a str,
    /// Only rows at or after this time.
    pub since: Option<Timestamp>,
    /// Look through Commands.
    pub commands: bool,
    /// Look through Events.
    pub events: bool,
}

/// Matching rows, newest first, before ranking.
#[derive(Debug, Default)]
pub struct SearchCandidates {
    /// Matching Commands.
    pub commands: Vec<Command>,
    /// Matching text-bearing Events.
    pub events: Vec<Event>,
}

/// The searchable text of an Event, if it has any. Command starts are left
/// out: their text is searched as a Command.
pub fn event_search_text(kind: &EventKind) -> Option<&str> {
    match kind {
        EventKind::ErrorDetected { message } | EventKind::WarningDetected { message } => {
            Some(message)
        }
        EventKind::WaitingForInput { prompt, .. } => prompt.as_deref(),
        EventKind::CheckpointCreated { path } => Some(path),
        EventKind::ServerStarted { url, .. } => url.as_deref(),
        _ => None,
    }
}

impl Store {
    /// Commands and Events that match `query`, newest first.
    pub fn search_candidates(
        &self,
        query: &SearchCandidateQuery<'_>,
    ) -> Result<SearchCandidates, StoreError> {
        let connection = self.lock()?;
        let since = query.since.map_or(i64::MIN, |at| at.0);
        let mut found = SearchCandidates::default();
        if query.commands {
            found.commands = command_candidates(&connection, query.text, since)?;
        }
        if query.events {
            found.events = event_candidates(&connection, query.text, since)?;
        }
        Ok(found)
    }
}

/// `"word" AND "word"` for the words an index can use, quoted so that FTS5
/// syntax in the owner's text is never interpreted. `None` when no word qualifies.
fn match_expression(text: &str, long: bool) -> Option<String> {
    let words: Vec<String> = text
        .split_whitespace()
        .filter(|word| (word.chars().count() >= TRIGRAM) == long)
        .map(|word| {
            let quoted = format!("\"{}\"", word.replace('"', "\"\""));
            if long {
                quoted
            } else {
                format!("{quoted}*")
            }
        })
        .collect();
    (!words.is_empty()).then(|| words.join(" AND "))
}

const COMMAND_COLUMNS: &str =
    "c.id, c.session_id, c.machine_id, c.text, c.cwd, c.started_at, c.finished_at, c.exit_code, c.class";

fn command_candidates(
    connection: &Connection,
    text: &str,
    since: i64,
) -> Result<Vec<Command>, StoreError> {
    let mut rows: Vec<Command> = Vec::new();
    let indexed = match_expression(text, true)
        .map(|expression| ("commands_trigram", expression))
        .or_else(|| match_expression(text, false).map(|e| ("commands_fts", e)));
    if let Some((table, expression)) = indexed {
        let mut statement = connection.prepare(&format!(
            "SELECT {COMMAND_COLUMNS} FROM {table} f JOIN commands c ON c.id = f.rowid
             WHERE {table} MATCH ?1 AND c.started_at >= ?2
             ORDER BY f.rowid DESC LIMIT ?3"
        ))?;
        let found = statement.query_map(
            params![expression, since, INDEXED_CANDIDATES],
            records::command_from_row,
        )?;
        rows.extend(found.collect::<Result<Vec<_>, _>>()?);
    }
    let mut statement = connection.prepare(&format!(
        "SELECT {COMMAND_COLUMNS} FROM commands c WHERE c.started_at >= ?1
         ORDER BY c.id DESC LIMIT ?2"
    ))?;
    let recent = statement.query_map(
        params![since, RECENT_SEARCH_WINDOW as u32],
        records::command_from_row,
    )?;
    rows.extend(recent.collect::<Result<Vec<_>, _>>()?);
    Ok(newest_matching(
        rows,
        |command| command.id.0,
        |command| match_score(text, &[&command.text]) > 0.0,
    ))
}

fn event_candidates(
    connection: &Connection,
    text: &str,
    since: i64,
) -> Result<Vec<Event>, StoreError> {
    let mut rows: Vec<Event> = Vec::new();
    if let Some(expression) = match_expression(text, true) {
        let mut statement = connection.prepare(
            "SELECT e.id, e.session_id, e.command_id, e.at, e.kind_json
             FROM events_trigram f JOIN events e ON e.id = f.rowid
             WHERE events_trigram MATCH ?1 AND e.at >= ?2
             ORDER BY f.rowid DESC LIMIT ?3",
        )?;
        let found = statement.query_map(
            params![expression, since, INDEXED_CANDIDATES],
            records::event_from_row,
        )?;
        rows.extend(found.collect::<Result<Vec<_>, _>>()?);
    }
    let mut statement = connection.prepare(
        "SELECT id, session_id, command_id, at, kind_json FROM (
             SELECT * FROM events ORDER BY id DESC LIMIT ?1
         ) WHERE at >= ?2 AND json_extract(kind_json, '$.type') IN
             ('ERROR_DETECTED', 'WARNING_DETECTED', 'WAITING_FOR_INPUT',
              'CHECKPOINT_CREATED', 'SERVER_STARTED')
         ORDER BY id DESC LIMIT ?3",
    )?;
    let recent = statement.query_map(
        params![RECENT_EVENT_SCAN, since, RECENT_SEARCH_WINDOW as u32],
        records::event_from_row,
    )?;
    rows.extend(recent.collect::<Result<Vec<_>, _>>()?);
    Ok(newest_matching(
        rows,
        |event| event.id.0,
        |event| event_search_text(&event.kind).is_some_and(|hay| match_score(text, &[hay]) > 0.0),
    ))
}

/// Distinct rows that match, newest (highest ID) first.
fn newest_matching<T>(
    mut rows: Vec<T>,
    id: impl Fn(&T) -> u64,
    matches: impl Fn(&T) -> bool,
) -> Vec<T> {
    rows.sort_by_key(|row| std::cmp::Reverse(id(row)));
    rows.dedup_by_key(|row| id(row));
    rows.retain(|row| matches(row));
    rows
}
