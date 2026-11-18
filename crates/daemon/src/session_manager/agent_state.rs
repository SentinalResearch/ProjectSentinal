//! An agent inside a Session reporting what it is doing (Milestones 6.4, 6.5).
//!
//! Accepted from a process the OS says runs inside that Session's process
//! tree, or from a `::sentinal task` / `state` line in the Session's own
//! output, only for live FULL Sessions. A report is presentation: it never
//! changes lifecycle state, is never written to the store, and clears when the
//! Command or Session ends.
//!
//! At most one report per 500 ms is applied. A report arriving sooner is held,
//! and the newest held report is applied when the window ends, so a quick
//! last step or `done` is never lost.

use super::{lock_session, SessionHandle, SessionManager, SessionManagerError};
use protocol::{
    AgentReportedState, AgentStatus, CaptureLevel, DaemonMessage, Session, SessionId, Timestamp,
};
use std::{collections::VecDeque, sync::Arc};

/// Shortest gap between two applied reports for one Session.
const MIN_REPORT_GAP_MS: i64 = 500;
/// Step times kept for the pace estimate: six points, five gaps.
const PACE_POINTS: usize = 6;

/// One report, from the CLI or a `::sentinal` line, before it is merged.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AgentReport {
    /// What the agent says it is doing; a `task` line leaves it as it was.
    pub status: Option<AgentStatus>,
    pub detail: Option<String>,
    pub task: Option<String>,
    pub step: Option<u32>,
    pub steps: Option<u32>,
    pub eta_secs: Option<u32>,
}

/// Per-Session rate limit, held report and step history.
#[derive(Debug, Default)]
pub(crate) struct AgentTrack {
    last_applied: Option<Timestamp>,
    held: Option<(AgentReport, Timestamp)>,
    timer_armed: bool,
    /// `(step, when it was first seen)`, oldest first.
    points: VecDeque<(u32, Timestamp)>,
}

impl SessionManager {
    /// Record what an agent running in `session_id` says it is doing.
    ///
    /// A report arriving within 500 ms of the last applied one is held and the
    /// Session is returned unchanged; the newest held report is applied when
    /// the window ends.
    pub(crate) fn report_agent_state(
        &self,
        session_id: SessionId,
        report: AgentReport,
        caller_pid: Option<u32>,
    ) -> Result<Session, SessionManagerError> {
        let caller = caller_pid.ok_or(SessionManagerError::AgentCallerUnknown)?;
        let handle = self.handle(session_id)?;
        let root = {
            let session = lock_session(&handle)?;
            if !takes_reports(&session) {
                return Err(SessionManagerError::AgentReportsNotTaken(session_id));
            }
            session.pid
        };
        // The process-tree walk can be slow; never hold the Session lock for it.
        let inside = root.is_some_and(|root| self.inner.probe.descends_from(caller, root));
        if !inside {
            return Err(SessionManagerError::AgentOutsideSession {
                pid: caller,
                session_id,
            });
        }
        submit(&handle, Some(&handle), report, (self.inner.config.clock)())
    }
}

fn takes_reports(session: &Session) -> bool {
    session.capture == CaptureLevel::Full && !session.state.is_finished()
}

/// Apply `report` now, or hold it if the last one was under 500 ms ago.
///
/// With `timer`, a held report is applied when the window ends; without it
/// (a `::sentinal` line), on the Session's next output ([`flush_due`]).
/// Takes the agent lock before the Session lock, never the other way round.
pub(super) fn submit(
    handle: &SessionHandle,
    timer: Option<&Arc<SessionHandle>>,
    report: AgentReport,
    at: Timestamp,
) -> Result<Session, SessionManagerError> {
    let id = handle.id;
    let mut track = handle.agent.lock().map_err(|_| SessionManagerError::Lock {
        resource: "agent reports",
        session_id: id,
    })?;
    if let Some(last) = track.last_applied {
        let gap = at.0.saturating_sub(last.0);
        if gap < MIN_REPORT_GAP_MS {
            track.held = Some((report, at));
            if let (false, Some(timer), Ok(runtime)) = (
                track.timer_armed,
                timer,
                tokio::runtime::Handle::try_current(),
            ) {
                {
                    track.timer_armed = true;
                    let wait = u64::try_from(MIN_REPORT_GAP_MS - gap).unwrap_or(0);
                    let handle = timer.clone();
                    runtime.spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                        flush_held(&handle);
                    });
                }
            }
            return Ok(lock_session(handle)?.clone());
        }
    }
    // A report applied now replaces anything held.
    track.held = None;
    apply(handle, &mut track, report, at)
}

/// Apply a held report whose 500 ms window has ended by `now`.
pub(super) fn flush_due(handle: &SessionHandle, now: Timestamp) {
    let due = handle.agent.lock().is_ok_and(|track| {
        !track.timer_armed
            && track.held.is_some()
            && track
                .last_applied
                .is_none_or(|last| now.0.saturating_sub(last.0) >= MIN_REPORT_GAP_MS)
    });
    if due {
        flush_held(handle);
    }
}

/// Apply the newest held report, if the Session still takes reports.
fn flush_held(handle: &SessionHandle) {
    let Ok(mut track) = handle.agent.lock() else {
        tracing::error!(session = handle.id.0, "agent report lock poisoned");
        return;
    };
    track.timer_armed = false;
    let Some((report, at)) = track.held.take() else {
        return;
    };
    let live = handle
        .session
        .lock()
        .is_ok_and(|session| takes_reports(&session));
    if !live {
        return;
    }
    if let Err(error) = apply(handle, &mut track, report, at) {
        tracing::debug!(session = handle.id.0, %error, "held agent report dropped");
    }
}

fn apply(
    handle: &SessionHandle,
    track: &mut AgentTrack,
    report: AgentReport,
    at: Timestamp,
) -> Result<Session, SessionManagerError> {
    let session = {
        let mut session = lock_session(handle)?;
        let merged = merge(
            session.agent_state.as_deref(),
            &mut track.points,
            report,
            at,
        )
        .map_err(SessionManagerError::AgentProgressInvalid)?;
        session.agent_state = Some(Box::new(merged));
        session.clone()
    };
    track.last_applied = Some(at);
    let _ = handle.structured.send(DaemonMessage::SessionUpdated {
        session: session.clone(),
    });
    Ok(session)
}

/// Merge `report` into the previous state (6.5).
///
/// - `status` and `detail` are replaced; a missing status keeps the old one.
/// - A missing `task`, `step` or `steps` keeps the old value.
/// - A different task starts fresh: new `task_started_at`, no old steps, ETA
///   or pace.
/// - The agent's ETA stands only while the step is unchanged.
/// - The pace is forgotten when the plan's step count changes or the step goes
///   backwards. With no previous state (the Command ended) it starts over.
pub(crate) fn merge(
    previous: Option<&AgentReportedState>,
    points: &mut VecDeque<(u32, Timestamp)>,
    report: AgentReport,
    at: Timestamp,
) -> Result<AgentReportedState, &'static str> {
    let Some(previous) = previous else {
        points.clear();
        return merge(
            Some(&AgentReportedState::new(AgentStatus::Working, at)),
            points,
            report,
            at,
        );
    };
    let new_task = report.task.is_some() && report.task != previous.task;
    let (step, steps, task_started_at) = if new_task {
        points.clear();
        (report.step, report.steps, Some(at))
    } else {
        let task_started_at = previous
            .task_started_at
            .or_else(|| previous.task.as_ref().map(|_| at));
        (
            report.step.or(previous.step),
            report.steps.or(previous.steps),
            task_started_at,
        )
    };
    let task = report.task.or_else(|| previous.task.clone());
    if let (Some(step), Some(steps)) = (step, steps) {
        if step > steps {
            return Err("step is past the last step");
        }
    }
    let same_step = !new_task && step == previous.step;
    let eta_secs = report
        .eta_secs
        .or(if same_step { previous.eta_secs } else { None });

    let plan_changed = !new_task && report.steps.is_some() && report.steps != previous.steps;
    if plan_changed {
        points.clear();
    }
    if let Some(step) = step {
        match points.back() {
            Some(&(last, _)) if step == last => {}
            Some(&(last, _)) if step < last => {
                points.clear();
                points.push_back((step, at));
            }
            _ => points.push_back((step, at)),
        }
        while points.len() > PACE_POINTS {
            points.pop_front();
        }
    }
    let estimated_left_secs = match (step, steps) {
        (Some(step), Some(steps)) => pace_ms(points)
            .map(|per_step| per_step.saturating_mul(u64::from(steps - step)) / 1_000)
            .map(|secs| u32::try_from(secs).unwrap_or(u32::MAX)),
        _ => None,
    };

    Ok(AgentReportedState {
        status: report.status.unwrap_or(previous.status),
        detail: report.detail,
        task,
        step,
        steps,
        eta_secs,
        task_started_at,
        estimated_left_secs,
        reported_at: at,
    })
}

/// The median time per step, in milliseconds, or `None` with fewer than two points.
fn pace_ms(points: &VecDeque<(u32, Timestamp)>) -> Option<u64> {
    let mut per_step: Vec<u64> = points
        .iter()
        .zip(points.iter().skip(1))
        .filter_map(|(&(s1, t1), &(s2, t2))| {
            let steps = u64::from(s2.checked_sub(s1)?);
            let ms = u64::try_from(t2.0.checked_sub(t1.0)?).ok()?;
            (steps > 0).then(|| ms / steps)
        })
        .collect();
    if per_step.is_empty() {
        return None;
    }
    per_step.sort_unstable();
    let mid = per_step.len() / 2;
    Some(if per_step.len() % 2 == 1 {
        per_step[mid]
    } else {
        (per_step[mid - 1] + per_step[mid]) / 2
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn working(step: u32, steps: u32) -> AgentReport {
        AgentReport {
            status: Some(AgentStatus::Working),
            step: Some(step),
            steps: Some(steps),
            ..AgentReport::default()
        }
    }

    #[test]
    fn with_no_previous_state_the_pace_starts_over() {
        let mut points = VecDeque::from([(1, Timestamp(0)), (2, Timestamp(10))]);
        let state = merge(None, &mut points, working(5, 8), Timestamp(100)).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(state.estimated_left_secs, None);
        assert_eq!(state.task_started_at, None, "no task, no time on task");
    }

    #[test]
    fn a_task_line_without_a_status_keeps_the_status() {
        let mut points = VecDeque::new();
        let blocked = AgentReportedState::new(AgentStatus::Blocked, Timestamp(1));
        let report = AgentReport {
            task: Some("migrate tests".into()),
            ..AgentReport::default()
        };
        let state = merge(Some(&blocked), &mut points, report, Timestamp(2)).unwrap();
        assert_eq!(state.status, AgentStatus::Blocked);
        assert_eq!(state.task_started_at, Some(Timestamp(2)));
    }

    #[test]
    fn only_the_newest_six_step_times_count() {
        let mut points = VecDeque::new();
        let mut previous = None;
        for step in 1..=10 {
            let state = merge(
                previous.as_ref(),
                &mut points,
                working(step, 20),
                Timestamp(i64::from(step) * 1_000),
            )
            .unwrap();
            previous = Some(state);
        }
        assert_eq!(points.len(), PACE_POINTS);
        assert_eq!(points.front().map(|p| p.0), Some(5));
        assert_eq!(previous.unwrap().estimated_left_secs, Some(10));
    }

    #[test]
    fn an_even_number_of_gaps_takes_the_middle_two() {
        let points = VecDeque::from([
            (1, Timestamp(0)),
            (2, Timestamp(1_000)),
            (3, Timestamp(4_000)),
        ]);
        assert_eq!(pace_ms(&points), Some(2_000));
    }
}
