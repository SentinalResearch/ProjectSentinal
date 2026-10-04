//! `sentinal top` and `sentinal status`: plain-text Machine and Session
//! metrics from the local daemon, readable over SSH without a browser.

use protocol::{
    ClientRequest, DaemonMessage, Event, EventKind, Machine, MachineDetails, MachineMetricPoint,
    MetricRange, Session, SessionId, SessionState, Timestamp,
};
use std::fmt::Write as _;

use crate::client::{ClientError, DaemonClient};

#[path = "top_format.rs"]
mod format;
use format::*;

/// Everything `top` shows, fetched in one round of requests.
#[derive(Debug, Clone)]
pub struct TopSnapshot {
    /// The daemon's Machine description.
    pub machine: Machine,
    /// Host, daemon, and per-Session process facts.
    pub details: MachineDetails,
    /// Most recent Machine-wide sample, if any.
    pub latest: Option<MachineMetricPoint>,
    /// Every Session on the daemon.
    pub sessions: Vec<Session>,
    /// When the snapshot was taken.
    pub now: Timestamp,
}

/// Fetch a `top` snapshot from the daemon.
pub async fn fetch(client: &mut DaemonClient, now: Timestamp) -> Result<TopSnapshot, ClientError> {
    let machine = match client.request(ClientRequest::GetMachine).await? {
        DaemonMessage::Machine { machine } => machine,
        other => return Err(unexpected(other)),
    };
    let details = match client.request(ClientRequest::GetMachineDetails).await? {
        DaemonMessage::MachineDetails { details } => details,
        other => return Err(unexpected(other)),
    };
    let latest = match client
        .request(ClientRequest::GetMachineMetrics {
            range: MetricRange::Hour,
        })
        .await?
    {
        DaemonMessage::MachineMetricsHistory { points, .. } => points.into_iter().last(),
        other => return Err(unexpected(other)),
    };
    let sessions = match client.request(ClientRequest::ListSessions).await? {
        DaemonMessage::Sessions { sessions } => sessions,
        other => return Err(unexpected(other)),
    };
    Ok(TopSnapshot {
        machine,
        details,
        latest,
        sessions,
        now,
    })
}

/// Recent Events of one Session, oldest first.
pub async fn fetch_events(
    client: &mut DaemonClient,
    session_id: SessionId,
) -> Result<Vec<Event>, ClientError> {
    match client
        .request(ClientRequest::GetEvents {
            session_id: Some(session_id),
            after: None,
            limit: 500,
        })
        .await?
    {
        DaemonMessage::Events { events } => Ok(events),
        other => Err(unexpected(other)),
    }
}

fn unexpected(message: DaemonMessage) -> ClientError {
    match message {
        DaemonMessage::Error { message } => ClientError::Daemon(message),
        other => ClientError::Daemon(format!("unexpected daemon response: {}", wire_name(&other))),
    }
}

/// The Machine header, load bars, and live Session table.
pub fn render_top(snapshot: &TopSnapshot) -> String {
    let mut text = String::new();
    let details = &snapshot.details;
    let _ = writeln!(
        text,
        "{} · {} · Sentinal {} · up {}",
        snapshot.machine.name,
        details.os_version.as_deref().unwrap_or("unknown OS"),
        details.daemon_version,
        duration(snapshot.now.0.saturating_sub(details.started_at.0)),
    );
    let _ = writeln!(text, "{}", hardware(&snapshot.machine));
    text.push('\n');
    match &snapshot.latest {
        Some(point) => {
            bar_line(
                &mut text,
                "CPU",
                Some(f64::from(point.cpu_percent)),
                format!("{:.1} %", point.cpu_percent),
            );
            bar_line(
                &mut text,
                "RAM",
                percent(point.ram_used_bytes, point.ram_total_bytes),
                format!(
                    "{} / {}",
                    gigabytes(point.ram_used_bytes),
                    gigabytes(point.ram_total_bytes)
                ),
            );
            if let Some(gpu) = point.gpu_percent {
                bar_line(
                    &mut text,
                    "GPU",
                    Some(f64::from(gpu)),
                    format!("{gpu:.1} %"),
                );
            }
            if let (Some(used), Some(total)) = (point.vram_used_bytes, point.vram_total_bytes) {
                bar_line(
                    &mut text,
                    "VRAM",
                    percent(used, total),
                    format!("{} / {}", gigabytes(used), gigabytes(total)),
                );
            }
        }
        None => text.push_str("No Machine samples yet.\n"),
    }
    text.push('\n');

    let live: Vec<&Session> = snapshot
        .sessions
        .iter()
        .filter(|session| is_live(session))
        .collect();
    let _ = writeln!(
        text,
        "SESSIONS ({} live of {})",
        live.len(),
        snapshot.sessions.len()
    );
    if live.is_empty() {
        text.push_str("Nothing is running.\n");
        return text;
    }
    let _ = writeln!(
        text,
        "{:<5} {:<12} {:<11} {:<22} {:<9} {:<22} {:>7} {:>9}  HEALTH",
        "ID", "STATE", "CAPTURE", "NAME", "PROGRESS", "LOSS", "CPU", "MEM"
    );
    for session in live {
        let (cpu, rss) = session_load(details, session.id);
        let _ = writeln!(
            text,
            "{:<5} {:<12} {:<11} {:<22} {:<9} {:<22} {:>7} {:>9}  {}",
            session.id.0,
            wire_name(&session.state),
            wire_name(&session.capture),
            clip(&session_name(session), 22),
            session
                .health
                .as_ref()
                .and_then(|health| health.progress)
                .map_or_else(|| "—".to_owned(), |progress| format!("{progress:.0} %")),
            loss_text(session),
            cpu.map_or_else(|| "—".to_owned(), |cpu| format!("{cpu:.1}%")),
            rss.map_or_else(|| "—".to_owned(), size),
            session
                .health
                .as_ref()
                .map_or_else(|| "—".to_owned(), |health| wire_name(&health.overall)),
        );
    }
    text
}

/// One Session in detail: progress, training, health evidence, processes, Events.
pub fn render_status(snapshot: &TopSnapshot, session: &Session, events: &[Event]) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "Session {} · {} · {} · {}",
        session.id.0,
        session_name(session),
        wire_name(&session.state),
        wire_name(&session.capture),
    );
    let _ = writeln!(
        text,
        "{} · {} · running {}",
        snapshot.machine.name,
        session.cwd,
        duration(snapshot.now.0.saturating_sub(session.created_at.0)),
    );
    text.push('\n');

    let progress = events.iter().rev().find_map(|event| match &event.kind {
        EventKind::ProgressUpdated {
            current,
            total,
            unit,
        } => Some((*current, *total, unit.clone())),
        _ => None,
    });
    if let Some((current, total, unit)) = progress {
        match total {
            Some(total) if total > 0 => {
                let share = current as f64 / total as f64 * 100.0;
                bar_line(
                    &mut text,
                    "PROGRESS",
                    Some(share),
                    format!("{unit} {current}/{total}"),
                );
            }
            _ => {
                let _ = writeln!(text, "PROGRESS  {unit} {current}");
            }
        }
    }
    if let Some(training) = &session.training {
        let mut parts = vec![format!("LOSS {}", loss_text(session))];
        if let Some(best) = training.best_val_loss {
            parts.push(format!("best val {best}"));
        }
        if let Some(rate) = training.learning_rate {
            parts.push(format!("lr {rate:e}"));
        }
        let _ = writeln!(text, "{}", parts.join(" · "));
    }
    let (cpu, rss) = session_load(&snapshot.details, session.id);
    if cpu.is_some() || rss.is_some() {
        let _ = writeln!(
            text,
            "CPU {} · MEM {}",
            cpu.map_or_else(|| "—".to_owned(), |cpu| format!("{cpu:.1} %")),
            rss.map_or_else(|| "—".to_owned(), size),
        );
    }
    if let Some(health) = &session.health {
        let _ = writeln!(
            text,
            "HEALTH {} · activity {} · resources {} · stability {} · {} error(s)",
            wire_name(&health.overall),
            wire_name(&health.activity),
            wire_name(&health.resources),
            wire_name(&health.stability),
            health.errors,
        );
        for evidence in latest_evidence(&health.evidence) {
            let _ = writeln!(text, "  - {evidence}");
        }
    }

    if let Some(entry) = snapshot
        .details
        .processes
        .iter()
        .find(|entry| entry.session_id == session.id)
    {
        text.push_str("\nPROCESSES\n");
        for process in &entry.processes {
            let _ = writeln!(
                text,
                "  {:<8} {:<20} {:>7.1}% {:>9}",
                process.pid,
                clip(&process.name, 20),
                process.cpu_percent,
                size(process.rss_bytes),
            );
        }
    }

    text.push_str("\nEVENTS\n");
    if events.is_empty() {
        text.push_str("  none yet\n");
    }
    for event in events
        .iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let _ = writeln!(text, "  {}  {}", clock(event.at), describe(&event.kind));
    }
    text
}

fn is_live(session: &Session) -> bool {
    !matches!(
        session.state,
        SessionState::Completed | SessionState::Failed | SessionState::Interrupted
    )
}

fn session_name(session: &Session) -> String {
    session
        .title
        .clone()
        .unwrap_or_else(|| session.shell.clone())
}

/// Summed CPU and memory of the Session's sampled processes.
fn session_load(details: &MachineDetails, session_id: SessionId) -> (Option<f32>, Option<u64>) {
    details
        .processes
        .iter()
        .find(|entry| entry.session_id == session_id)
        .map_or((None, None), |entry| {
            (
                Some(
                    entry
                        .processes
                        .iter()
                        .map(|process| process.cpu_percent)
                        .sum(),
                ),
                Some(
                    entry
                        .processes
                        .iter()
                        .map(|process| process.rss_bytes)
                        .sum(),
                ),
            )
        })
}

#[cfg(test)]
#[path = "top_tests.rs"]
mod tests;
