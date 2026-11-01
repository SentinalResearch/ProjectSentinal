//! Output search (Milestone 5.S): the saved terminal log of a FULL Session
//! the owner opted in, scanned on demand and never indexed. Matching lines
//! come back with ANSI removed and secret-shaped values redacted.

use protocol::{
    search::{
        clip, match_score, rank_score, SearchHit, SearchScope, SearchTarget, MATCH_SUBSTRING,
        MAX_HIT_DETAIL_CHARS, MAX_HIT_TITLE_CHARS,
    },
    Session, Timestamp,
};
use std::{collections::VecDeque, fs::File, io::Read, path::Path};
use watcher::{redact::redact_secrets, LineFramer};

/// Decompressed bytes read from one log per search.
pub const MAX_OUTPUT_SEARCH_BYTES: u64 = 64 * 1024 * 1024;
/// Latest matching lines kept per Session.
pub const OUTPUT_HITS_PER_SESSION: usize = 5;

/// The latest lines of `session`'s log at `path` that contain every word of `query`.
pub fn output_hits(session: &Session, path: &Path, query: &str, now: Timestamp) -> Vec<SearchHit> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(decoder) = zstd::stream::read::Decoder::new(file) else {
        return Vec::new();
    };
    let mut reader = decoder.take(MAX_OUTPUT_SEARCH_BYTES);
    let mut framer = LineFramer::default();
    let mut latest: VecDeque<(usize, String, f64)> = VecDeque::new();
    let mut number = 0;
    let mut buffer = [0_u8; 64 * 1024];
    let mut keep = |line: String| {
        number += 1;
        // Substring matches only: fuzzy matches over raw output are noise.
        let matched = match_score(query, &[&line]);
        if matched >= MATCH_SUBSTRING {
            latest.push_back((number, line, matched));
            if latest.len() > OUTPUT_HITS_PER_SESSION {
                latest.pop_front();
            }
        }
    };
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => framer
                .push(&buffer[..count])
                .into_iter()
                .for_each(&mut keep),
        }
    }
    if let Some(line) = framer.finish() {
        keep(line);
    }
    let name = session
        .title
        .clone()
        .unwrap_or_else(|| format!("Session {}", session.id.0));
    let age = now.0.saturating_sub(session.updated_at.0);
    latest
        .into_iter()
        .rev()
        .map(|(number, line, matched)| SearchHit {
            kind: SearchScope::Output,
            title: clip(&redact_secrets(&line), MAX_HIT_TITLE_CHARS),
            detail: clip(
                &format!("Output · {name} · line {number}"),
                MAX_HIT_DETAIL_CHARS,
            ),
            target: SearchTarget::Session {
                session_id: session.id,
            },
            at: Some(session.updated_at),
            score: rank_score(matched, age, false),
        })
        .collect()
}
