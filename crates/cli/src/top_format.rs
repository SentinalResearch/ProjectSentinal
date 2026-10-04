//! Plain-text formatting for `sentinal top` and `sentinal status`.

use protocol::{EventKind, Machine, Session, Timestamp};
use std::fmt::Write as _;

const BAR_WIDTH: usize = 20;
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

pub(super) fn loss_text(session: &Session) -> String {
    let Some(training) = &session.training else {
        return "—".to_owned();
    };
    let Some(loss) = training.loss else {
        return "—".to_owned();
    };
    let arrow = match training.trend {
        protocol::LossTrend::Improving => " ↓",
        protocol::LossTrend::Worsening => " ↑",
        protocol::LossTrend::Stable => " →",
        protocol::LossTrend::Unknown => "",
    };
    match training.first_loss {
        Some(first) if (first - loss).abs() > f64::EPSILON => {
            format!("{loss}{arrow} (from {first})")
        }
        _ => format!("{loss}{arrow}"),
    }
}

pub(super) fn describe(kind: &EventKind) -> String {
    let detail = match kind {
        EventKind::ProgressUpdated {
            current,
            total,
            unit,
        } => match total {
            Some(total) => format!("{unit} {current}/{total}"),
            None => format!("{unit} {current}"),
        },
        EventKind::CheckpointCreated { path } => path.clone(),
        EventKind::ResourceHigh { resource, percent } => {
            format!("{} {percent:.1} %", wire_name(resource))
        }
        EventKind::CommandFinished { exit_code, .. } => format!("exit {exit_code}"),
        EventKind::PossiblyStalled { silent_for_ms } => format!(
            "silent {}",
            duration(i64::try_from(silent_for_ms.0).unwrap_or(i64::MAX))
        ),
        EventKind::WaitingForInput { prompt } => prompt.clone().unwrap_or_default(),
        EventKind::ServerStarted { port, url } => url
            .clone()
            .or_else(|| port.map(|port| format!("port {port}")))
            .unwrap_or_default(),
        // Error and warning text comes from output; keep it to one short line.
        EventKind::ErrorDetected { message } | EventKind::WarningDetected { message } => {
            clip(message, 60)
        }
        _ => String::new(),
    };
    format!("{:<18} {detail}", event_type(kind))
        .trim_end()
        .to_owned()
}

pub(super) fn hardware(machine: &Machine) -> String {
    let mut parts = vec![machine.cpu.clone()];
    if machine.ram_bytes > 0 {
        parts.push(gigabytes(machine.ram_bytes));
    }
    for gpu in &machine.gpus {
        parts.push(match gpu.vram_bytes {
            Some(vram) => format!("{} {}", gpu.name, gigabytes(vram)),
            None => gpu.name.clone(),
        });
    }
    parts.join(", ")
}

pub(super) fn bar_line(text: &mut String, label: &str, share: Option<f64>, value: String) {
    let filled = share.map_or(0, |share| {
        ((share.clamp(0.0, 100.0) / 100.0) * BAR_WIDTH as f64).round() as usize
    });
    let _ = writeln!(
        text,
        "{label:<9} [{}{}] {value}",
        "█".repeat(filled),
        "░".repeat(BAR_WIDTH - filled)
    );
}

/// The newest line of each kind of evidence (numbers ignored), at most five.
pub(super) fn latest_evidence(evidence: &[String]) -> Vec<&str> {
    let kind = |line: &str| {
        line.chars()
            .filter(|character| !character.is_ascii_digit() && *character != '.')
            .collect::<String>()
    };
    let mut seen = std::collections::HashSet::new();
    let mut newest_first: Vec<&str> = evidence
        .iter()
        .rev()
        .filter(|line| seen.insert(kind(line)))
        .map(String::as_str)
        .take(5)
        .collect();
    newest_first.reverse();
    newest_first
}

pub(super) fn percent(used: u64, total: u64) -> Option<f64> {
    (total > 0).then(|| used as f64 / total as f64 * 100.0)
}

/// Process-sized memory: MB below 1 GB.
pub(super) fn size(bytes: u64) -> String {
    if (bytes as f64) < GIB {
        format!("{:.0} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        gigabytes(bytes)
    }
}

pub(super) fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GIB)
}

pub(super) fn duration(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match minutes {
        0 => format!("{}s", ms.max(0) / 1_000),
        1..=59 => format!("{minutes}m"),
        60..=2_879 => format!("{}h {}m", minutes / 60, minutes % 60),
        _ => format!("{}d {}h", minutes / 1_440, (minutes / 60) % 24),
    }
}

pub(super) fn clock(at: Timestamp) -> String {
    let seconds_of_day = at.0.div_euclid(1_000).rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02} UTC",
        seconds_of_day / 3_600,
        (seconds_of_day / 60) % 60,
        seconds_of_day % 60
    )
}

pub(super) fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// The SCREAMING_SNAKE_CASE wire name of an enum value, from its variant name.
pub(super) fn wire_name<T: std::fmt::Debug>(value: &T) -> String {
    let debug = format!("{value:?}");
    let variant: String = debug
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    let mut name = String::new();
    for (index, character) in variant.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            name.push('_');
        }
        name.push(character.to_ascii_uppercase());
    }
    name
}

pub(super) fn event_type(kind: &EventKind) -> String {
    wire_name(kind)
}
