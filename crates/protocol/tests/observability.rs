//! Contract for the 4.H health wire types.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{
    observability::{
        DaemonHealthReport, HealthIncident, HealthLabel, HealthMetric, HealthSampleBatch,
        HealthStat, IncidentKind,
    },
    Timestamp,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

fn fixture() -> Value {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/observability.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn round_trip<T: DeserializeOwned + Serialize>(value: &Value) -> T {
    let parsed: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        &serde_json::to_value(&parsed).unwrap(),
        value,
        "shape changed"
    );
    parsed
}

/// Exhaustive: a new label fails to compile here until the fixture covers it.
fn label_index(label: HealthLabel) -> usize {
    use HealthLabel::*;
    match label {
        LoadScript => 0,
        LoadSnapshot => 1,
        LoadLive => 2,
        LoadShown => 3,
        RouteSnapshot => 4,
        RouteRegistry => 5,
        RoutePing => 6,
        RouteTerminalSetup => 7,
        SocketDashboard => 8,
        SocketTerminal => 9,
        SocketMachine => 10,
        ErrorAttach => 11,
        ErrorInput => 12,
        ErrorCreate => 13,
        ErrorTimeout => 14,
        ErrorUnreachable => 15,
        ErrorAuthExpired => 16,
        ErrorUpload => 17,
        ErrorHeartbeat => 18,
        ErrorLogWrite => 19,
        ErrorStore => 20,
        ErrorPty => 21,
        ErrorOther => 22,
    }
}
const LABELS: usize = 23;

fn incident_index(kind: IncidentKind) -> usize {
    match kind {
        IncidentKind::Offline => 0,
        IncidentKind::Delayed => 1,
        IncidentKind::Relayed => 2,
        IncidentKind::Restarted => 3,
        IncidentKind::UpdateFailed => 4,
        IncidentKind::ErrorBurst => 5,
    }
}

#[test]
fn every_metric_and_label_has_a_valid_fixture_sample() {
    let batch: HealthSampleBatch = round_trip(&fixture()["sample_batch"]);
    let now = Timestamp(1_792_771_200_000);
    let mut metrics = std::collections::BTreeSet::new();
    let mut labels = [false; LABELS];
    for sample in &batch.samples {
        sample.validate(now).unwrap();
        metrics.insert(sample.metric);
        if let Some(label) = sample.label {
            labels[label_index(label)] = true;
        }
    }
    assert_eq!(
        metrics.len(),
        HealthMetric::ALL.len(),
        "a metric has no fixture"
    );
    assert!(labels.iter().all(|seen| *seen), "a label has no fixture");
}

#[test]
fn daemon_reports_incidents_and_stats_round_trip() {
    let fixture = fixture();
    let reports: Vec<DaemonHealthReport> = round_trip(&fixture["daemon_reports"]);
    assert_eq!(reports[1].upload_delay_ms, None);
    let incidents: Vec<HealthIncident> = round_trip(&fixture["incidents"]);
    let mut kinds = [false; 6];
    for incident in &incidents {
        kinds[incident_index(incident.kind)] = true;
        if let Some(detail) = &incident.detail {
            assert!(protocol::observability::valid_incident_detail(detail));
        }
    }
    assert!(
        kinds.iter().all(|seen| *seen),
        "an incident kind has no fixture"
    );
    let _: HealthStat = round_trip(&fixture["stat"]);
}

#[test]
fn unknown_metrics_and_labels_are_refused() {
    let text = r#"{"machine_id":"m","metric":"KEYSTROKES","at":1,"value":1}"#;
    assert!(serde_json::from_str::<protocol::observability::HealthSample>(text).is_err());
    let text = r#"{"machine_id":"m","metric":"ERRORS","label":"ERROR_SECRET","at":1,"value":1}"#;
    assert!(serde_json::from_str::<protocol::observability::HealthSample>(text).is_err());
}

#[test]
fn health_views_round_trip() {
    use protocol::health_report::{HealthIncidents, HealthReport, HealthSummary, ReportNames};
    let fixture = fixture();
    let summary: HealthSummary = round_trip(&fixture["summary"]);
    assert!(!summary.machines[1].reports_health);
    let _: HealthIncidents = round_trip(&fixture["incident_list"]);
    let report: HealthReport = round_trip(&fixture["report"]);
    assert_eq!(report.names, ReportNames::Anonymous);
    let text = fixture["report"].to_string();
    assert!(
        !text.contains("01J9"),
        "an anonymous report fixture carries no Machine ID"
    );
}
