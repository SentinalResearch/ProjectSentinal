//! Completion on one Machine (Milestone 5.A): its Command history ranked by
//! the frecency formula in `protocol::complete`, then its Recipes.

use crate::session_manager::{SessionManager, SessionManagerError};
use protocol::{
    complete::{completion_score, Completion, CompletionSource, CompletionWeights},
    Recipe, SessionId, Timestamp,
};
use std::collections::{BTreeMap, HashSet};
use store::{CompletionQuery, Store, StoreError};

/// Completion settings shared by the WebSocket and the local socket.
#[derive(Debug, Clone, Default)]
pub struct Completer {
    weights: CompletionWeights,
}

/// A `COMPLETE` request from either socket.
#[derive(Debug, Clone, Copy)]
pub struct CompleteArgs<'a> {
    /// The Session being typed into; its cwd applies when `cwd` is absent.
    pub session_id: Option<SessionId>,
    /// Text on the line so far.
    pub prefix: &'a str,
    /// The directory being typed in.
    pub cwd: Option<&'a str>,
    /// Also offer Commands that never succeeded.
    pub include_failed: bool,
    /// Most completions.
    pub limit: u32,
}

impl Completer {
    /// Use these ranking weights.
    pub fn new(weights: CompletionWeights) -> Self {
        Completer { weights }
    }

    /// Answer `args` from this Machine's history and `recipes`.
    pub fn answer(
        &self,
        sessions: &SessionManager,
        recipes: &[Recipe],
        args: &CompleteArgs<'_>,
    ) -> Result<Vec<Completion>, SessionManagerError> {
        let session_cwd = match (args.cwd, args.session_id) {
            (None, Some(id)) => Some(sessions.session(id)?.cwd),
            _ => None,
        };
        let cwd = args.cwd.or(session_cwd.as_deref());
        sessions.complete(
            recipes,
            &self.weights,
            &CompleteRequest {
                prefix: args.prefix,
                cwd,
                project_root: None,
                include_failed: args.include_failed,
                limit: args.limit,
            },
        )
    }
}

/// One completion, as asked.
#[derive(Debug, Clone, Copy)]
pub struct CompleteRequest<'a> {
    /// Text on the line so far.
    pub prefix: &'a str,
    /// The directory being typed in.
    pub cwd: Option<&'a str>,
    /// Root of the current project, if the directory is in one.
    pub project_root: Option<&'a str>,
    /// Also offer Commands that never succeeded.
    pub include_failed: bool,
    /// Most completions.
    pub limit: u32,
}

/// Rank this Machine's completions for `request`, best first.
pub fn complete(
    store: &Store,
    recipes: &[Recipe],
    weights: &CompletionWeights,
    request: &CompleteRequest<'_>,
    now: Timestamp,
) -> Result<Vec<Completion>, StoreError> {
    let rows = store.completion_candidates(&CompletionQuery {
        prefix: request.prefix,
        cwd: request.cwd,
        project_root: request.project_root,
    })?;
    let mut completions: Vec<Completion> = rows
        .into_iter()
        .filter_map(|(text, stats)| {
            let score = completion_score(weights, &stats, now, request.include_failed);
            (score > 0.0).then(|| Completion {
                count: if request.include_failed {
                    stats.succeeded + stats.failed
                } else {
                    stats.succeeded
                },
                text,
                source: CompletionSource::History,
                last_at: Some(stats.last_at),
                last_exit: None,
                score,
            })
        })
        .collect();
    let known: HashSet<String> = completions.iter().map(|c| c.text.clone()).collect();
    let mut offered = HashSet::new();
    for recipe in recipes {
        let Some(text) = recipe_line(recipe) else {
            continue;
        };
        if text.starts_with(request.prefix)
            && text != request.prefix
            && !known.contains(&text)
            && offered.insert(text.clone())
        {
            completions.push(Completion {
                text,
                source: CompletionSource::Recipe,
                count: 0,
                last_at: None,
                last_exit: None,
                score: weights.recipe,
            });
        }
    }
    completions.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.last_at.cmp(&a.last_at))
            .then_with(|| a.text.cmp(&b.text))
    });
    completions.truncate(request.limit as usize);
    // Only the few shown need their last exit code.
    for completion in &mut completions {
        if completion.source == CompletionSource::History {
            completion.last_exit = store.last_finished_exit(&completion.text)?;
        }
    }
    Ok(completions)
}

/// A Recipe's command with its defaults, or up to its first required value.
fn recipe_line(recipe: &Recipe) -> Option<String> {
    if let Ok(line) = recipe.render(&BTreeMap::new()) {
        return Some(line);
    }
    let required = recipe
        .params
        .iter()
        .filter(|param| param.default.is_none())
        .filter_map(|param| recipe.command.find(&format!("{{{}}}", param.name)))
        .min()?;
    let head = &recipe.command[..required];
    // Only a plain head: no other placeholder before the first required one.
    (!head.contains('{') && !head.trim().is_empty()).then(|| head.to_string())
}
