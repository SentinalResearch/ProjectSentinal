//! Privacy-safe ntfy presentation derived only from structured Events.

use protocol::{EventKind, MachineId, NotificationCategory, Resource, SessionId};

const MAX_LABEL_CHARS: usize = 48;
const MAX_TITLE_BYTES: usize = 100;
const MAX_BODY_BYTES: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Presentation {
    pub(super) category: NotificationCategory,
    pub(super) title: String,
    pub(super) body: String,
    pub(super) tag: &'static str,
    pub(super) priority: &'static str,
    pub(super) click: Option<String>,
}

pub(super) fn category(kind: &EventKind) -> Option<NotificationCategory> {
    match kind {
        EventKind::CommandFinished { exit_code: 0, .. } => Some(NotificationCategory::Completed),
        EventKind::CommandFinished { .. } => Some(NotificationCategory::Failed),
        EventKind::ErrorDetected { .. } => Some(NotificationCategory::Error),
        EventKind::WaitingForInput { .. } => Some(NotificationCategory::Waiting),
        EventKind::PossiblyStalled { .. } => Some(NotificationCategory::Stalled),
        EventKind::ResourceHigh { .. } => Some(NotificationCategory::ResourceHigh),
        EventKind::CheckpointCreated { .. } => Some(NotificationCategory::Checkpoint),
        EventKind::ProgressUpdated { .. } => Some(NotificationCategory::Progress),
        _ => None,
    }
}

pub(super) fn render(
    kind: &EventKind,
    machine: &str,
    session_title: Option<&str>,
    machine_id: &MachineId,
    session_id: SessionId,
    control_origin: Option<&str>,
) -> Option<Presentation> {
    let category = category(kind)?;
    let machine = safe_label(machine, "Machine");
    let fallback = format!("Session {}", session_id.0);
    let session = safe_label(session_title.unwrap_or(&fallback), &fallback);
    let (detail, tag, priority) = details(kind, category);
    let title = truncate_utf8(&format!("Sentinal · {machine}"), MAX_TITLE_BYTES);
    let body = truncate_utf8(&format!("{session} — {detail}"), MAX_BODY_BYTES);
    let click = control_origin.map(|origin| {
        let composite = format!("{}:{}", machine_id.0, session_id.0);
        format!(
            "{}/app/sessions/{}",
            origin.trim_end_matches('/'),
            percent_encode_component(&composite)
        )
    });
    Some(Presentation {
        category,
        title,
        body,
        tag,
        priority,
        click,
    })
}

fn details(
    kind: &EventKind,
    category: NotificationCategory,
) -> (String, &'static str, &'static str) {
    match (category, kind) {
        (NotificationCategory::Completed, EventKind::CommandFinished { exit_code, .. }) => (
            format!("completed (exit {exit_code})"),
            "white_check_mark",
            "default",
        ),
        (NotificationCategory::Failed, EventKind::CommandFinished { exit_code, .. }) => {
            (format!("failed (exit {exit_code})"), "x", "high")
        }
        (NotificationCategory::Error, _) => ("error detected".into(), "rotating_light", "high"),
        (NotificationCategory::Waiting, _) => {
            ("waiting for input".into(), "hourglass_flowing_sand", "high")
        }
        (NotificationCategory::Stalled, EventKind::PossiblyStalled { silent_for_ms }) => (
            format!("possibly stalled ({} min)", silent_for_ms.0 / 60_000),
            "warning",
            "default",
        ),
        (NotificationCategory::ResourceHigh, EventKind::ResourceHigh { resource, percent }) => (
            format!("{} usage high ({percent:.0}%)", resource_name(*resource)),
            "fire",
            "high",
        ),
        (NotificationCategory::Checkpoint, _) => {
            ("checkpoint created".into(), "floppy_disk", "low")
        }
        (
            NotificationCategory::Progress,
            EventKind::ProgressUpdated {
                current,
                total,
                unit,
            },
        ) => {
            let unit = safe_label(unit, "step");
            let value =
                total.map_or_else(|| current.to_string(), |total| format!("{current}/{total}"));
            (format!("{unit} {value}"), "chart_with_upwards_trend", "low")
        }
        _ => ("needs attention".into(), "warning", "default"),
    }
}

fn resource_name(resource: Resource) -> &'static str {
    match resource {
        Resource::Cpu => "CPU",
        Resource::Ram => "RAM",
        Resource::Gpu => "GPU",
        Resource::Vram => "VRAM",
        Resource::Disk => "disk",
    }
}

fn safe_label(value: &str, fallback: &str) -> String {
    let mut output = String::new();
    let mut pending_space = false;
    for character in value.trim().chars() {
        if character.is_control()
            || ('\u{202a}'..='\u{202e}').contains(&character)
            || ('\u{2066}'..='\u{2069}').contains(&character)
        {
            continue;
        }
        if character.is_whitespace() {
            pending_space = !output.is_empty();
            continue;
        }
        if pending_space && output.chars().count() < MAX_LABEL_CHARS {
            output.push(' ');
        }
        pending_space = false;
        if output.chars().count() >= MAX_LABEL_CHARS {
            break;
        }
        output.push(character);
    }
    if output.is_empty() {
        fallback.to_string()
    } else {
        output
    }
}

fn truncate_utf8(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_string();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].trim_end().to_string()
}

fn percent_encode_component(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(*byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}
