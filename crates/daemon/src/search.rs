//! One Machine's search (Milestone 5.S): Sessions, Commands, Events, and
//! Recipes from this Machine's own store, ranked with the shared formula in
//! `protocol::search`. Command and Event text never leaves the Machine except
//! in a reply to the owner's own request, like History.

use protocol::{
    search::{
        clip, match_score, rank_score, SearchFilters, SearchHit, SearchOutcome, SearchScope,
        SearchTarget, MAX_HIT_DETAIL_CHARS, MAX_HIT_TITLE_CHARS,
    },
    Command, Event, EventKind, Recipe, Session, SessionId, SessionState, Timestamp,
};
use std::collections::HashMap;
use store::{event_search_text, SearchCandidateQuery, Store, StoreError};

/// One search, as asked.
#[derive(Debug, Clone)]
pub struct SearchRequest<'a> {
    /// What the owner typed.
    pub query: &'a str,
    /// Where to look; empty means everywhere.
    pub scopes: &'a [SearchScope],
    /// Optional narrowing.
    pub filters: SearchFilters,
    /// Most hits to return.
    pub limit: u32,
}

impl SearchRequest<'_> {
    fn wants(&self, scope: SearchScope) -> bool {
        self.scopes.is_empty() || self.scopes.contains(&scope)
    }
}

/// Search this Machine and return its best hits, best first.
pub fn search(
    store: &Store,
    recipes: &[Recipe],
    request: &SearchRequest<'_>,
    now: Timestamp,
) -> Result<Vec<SearchHit>, StoreError> {
    let sessions = store.sessions()?;
    let titles: HashMap<SessionId, String> = sessions
        .iter()
        .map(|session| (session.id, session_name(session)))
        .collect();
    let candidates = store.search_candidates(&SearchCandidateQuery {
        text: request.query,
        since: request.filters.since,
        commands: request.wants(SearchScope::Commands),
        events: request.wants(SearchScope::Events),
    })?;
    let mut hits = Vec::new();
    if request.wants(SearchScope::Sessions) {
        hits.extend(
            sessions
                .iter()
                .filter_map(|session| session_hit(session, request, now)),
        );
    }
    hits.extend(
        candidates
            .commands
            .iter()
            .filter_map(|command| command_hit(command, request, now)),
    );
    hits.extend(
        candidates
            .events
            .iter()
            .filter_map(|event| event_hit(event, &titles, request, now)),
    );
    if request.wants(SearchScope::Recipes) && request.filters.is_empty() {
        hits.extend(
            recipes
                .iter()
                .filter_map(|recipe| recipe_hit(recipe, request.query)),
        );
    }
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.at.map(|at| at.0).cmp(&a.at.map(|at| at.0)))
    });
    hits.truncate(request.limit as usize);
    Ok(hits)
}

fn session_name(session: &Session) -> String {
    session
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| format!("Session {}", session.id.0))
}

fn state_word(state: SessionState) -> String {
    let name = format!("{state:?}");
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| format!("{first}{}", chars.as_str().to_lowercase()))
        .unwrap_or_default()
}

fn age(now: Timestamp, at: Timestamp) -> i64 {
    now.0.saturating_sub(at.0)
}

fn hit(
    kind: SearchScope,
    title: &str,
    detail: &str,
    target: SearchTarget,
    at: Option<Timestamp>,
    score: f64,
) -> SearchHit {
    SearchHit {
        kind,
        title: clip(title, MAX_HIT_TITLE_CHARS),
        detail: clip(detail, MAX_HIT_DETAIL_CHARS),
        target,
        at,
        score,
    }
}

fn session_hit(
    session: &Session,
    request: &SearchRequest<'_>,
    now: Timestamp,
) -> Option<SearchHit> {
    let at = session.updated_at;
    if request.filters.since.is_some_and(|since| at.0 < since.0) {
        return None;
    }
    let outcome_ok = match request.filters.outcome {
        None => true,
        Some(SearchOutcome::Failed) => session.state == SessionState::Failed,
        Some(SearchOutcome::Succeeded) => session.state == SessionState::Completed,
        Some(SearchOutcome::Running) => session.state.is_busy(),
    };
    let name = session_name(session);
    let matched = match_score(request.query, &[&name, &session.cwd]);
    let score = rank_score(
        matched,
        age(now, at),
        session.state == SessionState::Waiting,
    );
    (outcome_ok && score > 0.0).then(|| {
        hit(
            SearchScope::Sessions,
            &name,
            &format!("{} · {}", state_word(session.state), session.cwd),
            SearchTarget::Session {
                session_id: session.id,
            },
            Some(at),
            score,
        )
    })
}

fn command_hit(
    command: &Command,
    request: &SearchRequest<'_>,
    now: Timestamp,
) -> Option<SearchHit> {
    let outcome_ok = match request.filters.outcome {
        None => true,
        Some(SearchOutcome::Failed) => command.exit_code.is_some_and(|code| code != 0),
        Some(SearchOutcome::Succeeded) => command.exit_code == Some(0),
        Some(SearchOutcome::Running) => command.finished_at.is_none(),
    };
    let score = rank_score(
        match_score(request.query, &[&command.text]),
        age(now, command.started_at),
        false,
    );
    let state = match (command.finished_at, command.exit_code) {
        (None, _) => "running".to_string(),
        (Some(_), Some(code)) => format!("exit {code}"),
        (Some(_), None) => "finished".to_string(),
    };
    (outcome_ok && score > 0.0).then(|| {
        hit(
            SearchScope::Commands,
            &command.text,
            &format!("{state} · {}", command.cwd),
            SearchTarget::Command {
                session_id: command.session_id,
                command_id: command.id,
            },
            Some(command.started_at),
            score,
        )
    })
}

fn event_word(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::ErrorDetected { .. } => "Error",
        EventKind::WarningDetected { .. } => "Warning",
        EventKind::WaitingForInput { .. } => "Prompt",
        EventKind::CheckpointCreated { .. } => "Checkpoint",
        EventKind::ServerStarted { .. } => "Server",
        _ => "Event",
    }
}

fn event_hit(
    event: &Event,
    titles: &HashMap<SessionId, String>,
    request: &SearchRequest<'_>,
    now: Timestamp,
) -> Option<SearchHit> {
    let text = event_search_text(&event.kind)?;
    let outcome_ok = match request.filters.outcome {
        None => true,
        Some(SearchOutcome::Failed) => matches!(event.kind, EventKind::ErrorDetected { .. }),
        Some(SearchOutcome::Succeeded | SearchOutcome::Running) => false,
    };
    let score = rank_score(
        match_score(request.query, &[text]),
        age(now, event.at),
        false,
    );
    let session = titles
        .get(&event.session_id)
        .cloned()
        .unwrap_or_else(|| format!("Session {}", event.session_id.0));
    (outcome_ok && score > 0.0).then(|| {
        hit(
            SearchScope::Events,
            &format!("{} · {session}", event_word(&event.kind)),
            text,
            SearchTarget::Event {
                session_id: event.session_id,
                event_id: event.id,
            },
            Some(event.at),
            score,
        )
    })
}

fn recipe_hit(recipe: &Recipe, query: &str) -> Option<SearchHit> {
    let category = recipe.category.as_deref().unwrap_or("");
    let matched = match_score(
        query,
        &[&recipe.name, &recipe.id, category, &recipe.command],
    );
    // A Recipe has no time: no recency boost.
    let score = rank_score(matched, i64::MAX, false);
    (score > 0.0).then(|| {
        hit(
            SearchScope::Recipes,
            &recipe.name,
            &recipe.command,
            SearchTarget::Recipe {
                recipe_id: recipe.id.clone(),
            },
            None,
            score,
        )
    })
}
