//! Updating one worker from the dashboard, on the control (4.E §11). The
//! control offers only the release on its own stable channel, forwards a
//! validated version over the authenticated link, and keeps each worker's
//! latest report in memory (refreshed by every heartbeat).

use super::{proxy::worker_url, ControlFailure, ControlService};
use crate::link_audit::LinkAuditRecord;
use protocol::{
    link::{LinkFailure, LinkFailureCode},
    worker_update::{WorkerUpdateRequest, WorkerUpdateState, WorkerUpdateStatus},
    MachineId, Timestamp,
};
use std::time::Duration;

/// A started update with no report for this long shows as UNKNOWN.
const UNKNOWN_AFTER_MS: i64 = 5 * 60 * 1000;

/// Client for control → worker update calls.
pub(super) fn worker_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

impl ControlService {
    /// The version on this control's stable channel, when it publishes packages.
    pub fn available_release(&self) -> Option<String> {
        let path = self
            .settings
            .package_dir
            .as_ref()?
            .join("channels/stable.json");
        let text = std::fs::read_to_string(path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let version = value.get("version")?.as_str()?;
        protocol::release::strict_version(version).then(|| version.to_string())
    }

    /// Ask one worker to update to the stable release.
    pub async fn start_worker_update(
        &self,
        machine_id: &MachineId,
        request: &WorkerUpdateRequest,
    ) -> Result<WorkerUpdateStatus, ControlFailure> {
        request.validate().map_err(|error| {
            ControlFailure::new(400, LinkFailureCode::InvalidRequest, error.to_string())
        })?;
        if self.available_release().as_deref() != Some(request.version.as_str()) {
            return Err(ControlFailure::new(
                409,
                LinkFailureCode::InvalidRequest,
                format!(
                    "{} is not the release on this control's stable channel",
                    request.version
                ),
            ));
        }
        let target = self.routing_target(machine_id)?;
        let response = self
            .worker_http
            .post(worker_url(&target, "http", "/api/v1/link/update"))
            .bearer_auth(target.control_credential.expose())
            .json(request)
            .send()
            .await
            .map_err(|_| {
                ControlFailure::new(
                    502,
                    LinkFailureCode::Internal,
                    "the worker could not be reached over Tailscale",
                )
            })?;
        let status = response.status();
        let body = response.bytes().await.unwrap_or_default();
        let now = (self.clock)();
        if status == reqwest::StatusCode::ACCEPTED {
            let started: WorkerUpdateStatus = serde_json::from_slice(&body).map_err(|_| {
                ControlFailure::new(
                    502,
                    LinkFailureCode::Internal,
                    "the worker sent an unexpected reply",
                )
            })?;
            self.record_worker_update(machine_id, Some(started.clone()));
            self.audit_update(now, machine_id, "WORKER_UPDATE_STARTED", &request.version);
            return Ok(started);
        }
        let failure = serde_json::from_slice::<LinkFailure>(&body).unwrap_or(LinkFailure {
            code: LinkFailureCode::Internal,
            message: format!("the worker refused the update (HTTP {status})"),
        });
        if failure.code != LinkFailureCode::LiveSessions {
            self.audit_update(now, machine_id, "WORKER_UPDATE_REFUSED", &request.version);
        }
        let status = match status.as_u16() {
            401 => 502,
            code @ 400..=499 => code,
            _ => 502,
        };
        Err(ControlFailure { status, failure })
    }

    /// Keep a worker's latest report (a heartbeat without one keeps the last).
    pub(super) fn record_worker_update(
        &self,
        machine_id: &MachineId,
        status: Option<WorkerUpdateStatus>,
    ) {
        let Some(status) = status else { return };
        if let Ok(mut updates) = self.worker_updates.lock() {
            let previous = updates.insert(machine_id.clone(), status.clone());
            let finished = matches!(
                status.state,
                WorkerUpdateState::Installed | WorkerUpdateState::Failed
            );
            if finished && previous.is_some_and(|previous| previous.state != status.state) {
                let kind = if status.state == WorkerUpdateState::Installed {
                    "WORKER_UPDATE_INSTALLED"
                } else {
                    "WORKER_UPDATE_FAILED"
                };
                self.audit_update((self.clock)(), machine_id, kind, &status.version);
            }
        }
    }

    /// The latest update for the registry; a silent start becomes UNKNOWN.
    pub(super) fn worker_update_view(
        &self,
        machine_id: &MachineId,
        now: Timestamp,
    ) -> Option<WorkerUpdateStatus> {
        let mut status = self.worker_updates.lock().ok()?.get(machine_id).cloned()?;
        if status.state == WorkerUpdateState::Started
            && now.0 - status.started_at.0 > UNKNOWN_AFTER_MS
        {
            status.state = WorkerUpdateState::Unknown;
        }
        Some(status)
    }

    fn audit_update(&self, now: Timestamp, machine_id: &MachineId, kind: &str, version: &str) {
        self.audit.append(&LinkAuditRecord {
            machine_id: Some(&machine_id.0),
            reason: Some(version),
            ..LinkAuditRecord::new(now, kind)
        });
    }
}

/// `POST /api/v1/machines/{machine_id}/update` (owner browser Session only).
pub(super) fn routes() -> axum::Router<super::routes::ControlRoutes> {
    axum::Router::new().route(
        "/api/v1/machines/{machine_id}/update",
        axum::routing::post(update_route),
    )
}

async fn update_route(
    axum::extract::State(routes): axum::extract::State<super::routes::ControlRoutes>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(machine_id): axum::extract::Path<String>,
    axum::Json(request): axum::Json<WorkerUpdateRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let control = match routes.owner(&headers) {
        Ok(control) => control.clone(),
        Err(failure) => return super::routes::failure_response(failure),
    };
    match control
        .start_worker_update(&MachineId(machine_id), &request)
        .await
    {
        Ok(status) => (axum::http::StatusCode::ACCEPTED, axum::Json(status)).into_response(),
        Err(failure) => super::routes::failure_response(failure),
    }
}
