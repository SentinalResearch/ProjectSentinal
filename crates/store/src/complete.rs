//! Completion history (Milestone 5.A): each distinct Command text that
//! starts with the prefix, summed up for the frecency formula in
//! `protocol::complete`.

use crate::{Store, StoreError};
use protocol::{complete::CandidateStats, Timestamp};
use rusqlite::params;

/// Distinct texts read for one completion; plenty for a ranked top 20.
const MAX_TEXTS: u32 = 2_000;
/// Short prefixes look only at this many recent Commands.
const SHORT_PREFIX_WINDOW: i64 = 20_000;
/// An empty line looks at fewer: it only needs recent habits.
const EMPTY_PREFIX_WINDOW: i64 = 5_000;
/// Prefixes shorter than this (in characters) use the recent window.
const SHORT_PREFIX: usize = 3;

/// What to complete.
#[derive(Debug, Clone, Copy)]
pub struct CompletionQuery<'a> {
    /// Text on the line so far; matched exactly, case included.
    pub prefix: &'a str,
    /// The directory being typed in.
    pub cwd: Option<&'a str>,
    /// Root of the current project.
    pub project_root: Option<&'a str>,
}

impl Store {
    /// `(text, stats)` for each distinct one-line Command text that starts
    /// with, and is longer than, the prefix.
    pub fn completion_candidates(
        &self,
        query: &CompletionQuery<'_>,
    ) -> Result<Vec<(String, CandidateStats)>, StoreError> {
        let connection = self.lock()?;
        let upper = format!("{}\u{10FFFF}", query.prefix);
        let window = match query.prefix.chars().count() {
            0 => EMPTY_PREFIX_WINDOW,
            short if short < SHORT_PREFIX => SHORT_PREFIX_WINDOW,
            _ => i64::MAX,
        };
        // `root/` so a sibling such as `/home/me/ml-old` is not inside `/home/me/ml`.
        let root = query.project_root.map(|root| root.trim_end_matches('/'));
        let root_slash = root.map(|root| format!("{root}/"));
        let mut statement = connection.prepare(
            "SELECT text,
                    COALESCE(SUM(exit_code = 0), 0),
                    COALESCE(SUM(exit_code IS NOT NULL AND exit_code != 0), 0),
                    MAX(started_at),
                    COALESCE(SUM(exit_code IS NOT NULL AND ?3 IS NOT NULL
                        AND (cwd = ?3 OR substr(cwd, 1, length(?4)) = ?4)), 0),
                    COALESCE(SUM(exit_code IS NOT NULL AND cwd = ?5), 0)
             FROM commands
             WHERE text >= ?1 AND text < ?2 AND text != ?1
               AND instr(text, char(10)) = 0
               AND id > (SELECT COALESCE(MAX(id), 0) FROM commands) - ?6
             GROUP BY text
             ORDER BY MAX(started_at) DESC
             LIMIT ?7",
        )?;
        let rows = statement.query_map(
            params![
                query.prefix,
                upper,
                root,
                root_slash,
                query.cwd,
                window,
                MAX_TEXTS
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    CandidateStats {
                        succeeded: row.get(1)?,
                        failed: row.get(2)?,
                        last_at: Timestamp(row.get(3)?),
                        in_project: row.get(4)?,
                        in_cwd: row.get(5)?,
                    },
                ))
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Exit code of the latest finished run of exactly `text`.
    pub fn last_finished_exit(&self, text: &str) -> Result<Option<i32>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare_cached(
            "SELECT exit_code FROM commands WHERE text = ?1 AND exit_code IS NOT NULL
             ORDER BY started_at DESC, id DESC LIMIT 1",
        )?;
        let mut rows = statement.query([text])?;
        Ok(match rows.next()? {
            Some(row) => row.get(0)?,
            None => None,
        })
    }
}
