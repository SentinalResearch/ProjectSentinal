//! `sentinal doctor --report`: on the control, save the 4.H health report
//! (the same document Settings → Diagnostics downloads) to a file.

use protocol::{
    health_report::{HealthRange, ReportNames},
    LocalErrorCode, LocalRequest, LocalResponse,
};
use std::{io::Write, path::PathBuf, process::ExitCode};

/// Parse `--range`: `1h`, `24h` (default), `7d`, or `30d`.
pub(crate) fn parse_range(text: Option<&str>) -> Result<HealthRange, String> {
    let text = text.unwrap_or("24h");
    serde_json::from_value(serde_json::Value::String(text.into()))
        .map_err(|_| format!("unknown range '{text}'; use 1h, 24h, 7d, or 30d"))
}

/// Parse `--names`: `anonymous` (default) or `real`.
pub(crate) fn parse_names(text: Option<&str>) -> Result<ReportNames, String> {
    let text = text.unwrap_or("anonymous");
    serde_json::from_value(serde_json::Value::String(text.into()))
        .map_err(|_| format!("unknown names '{text}'; use anonymous or real"))
}

/// What to tell the owner for the daemon's answer.
pub(crate) fn outcome(response: LocalResponse) -> Result<PathBuf, String> {
    match response {
        LocalResponse::HealthReportWritten { path, .. } => Ok(PathBuf::from(path)),
        LocalResponse::Error {
            code: LocalErrorCode::InvalidRequest,
            ..
        } => Err(
            "reports come from the control. Run `sentinal doctor --report` on the control, \
             or download one from Settings → Diagnostics"
                .into(),
        ),
        LocalResponse::Error { message, .. } => Err(message),
        other => Err(format!("unexpected daemon response: {other:?}")),
    }
}

/// Ask the local daemon for the report and copy it to `output`.
pub fn run(
    range: Option<String>,
    names: Option<String>,
    output: Option<PathBuf>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let result = (|| {
        let range = parse_range(range.as_deref())?;
        let names = parse_names(names.as_deref())?;
        let written = outcome(crate::setup::local_socket(
            LocalRequest::WriteHealthReport { range, names },
        )?)?;
        let label = serde_json::to_value(range)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        let target =
            output.unwrap_or_else(|| PathBuf::from(format!("sentinal-health-{label}.json")));
        std::fs::copy(&written, &target)
            .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
        Ok::<_, String>(target)
    })();
    match result {
        Ok(target) => {
            let _ = writeln!(
                out,
                "Health report saved to {}. Numbers and codes only; safe to share unless you chose --names real.",
                target.display()
            );
            ExitCode::SUCCESS
        }
        Err(message) => {
            let _ = writeln!(err, "sentinal doctor --report: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_and_names_parse_with_defaults() {
        assert_eq!(parse_range(None), Ok(HealthRange::Day));
        assert_eq!(parse_range(Some("7d")), Ok(HealthRange::Week));
        assert!(parse_range(Some("2y"))
            .unwrap_err()
            .contains("1h, 24h, 7d, or 30d"));
        assert_eq!(parse_names(None), Ok(ReportNames::Anonymous));
        assert_eq!(parse_names(Some("real")), Ok(ReportNames::Real));
        assert!(parse_names(Some("hostnames")).is_err());
    }

    #[test]
    fn a_worker_points_to_the_control() {
        let refused = outcome(LocalResponse::Error {
            code: LocalErrorCode::InvalidRequest,
            message: "health reports come from the control".into(),
        });
        assert!(refused
            .unwrap_err()
            .starts_with("reports come from the control"));
        let written = outcome(LocalResponse::HealthReportWritten {
            path: "/data/health-report-24h.json".into(),
            bytes: 10,
        });
        assert_eq!(written, Ok(PathBuf::from("/data/health-report-24h.json")));
    }
}
