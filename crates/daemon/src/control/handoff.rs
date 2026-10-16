//! A control stepping down hands its workers to another control. It tells
//! each active worker, over its authenticated link, which control to ask; the
//! worker then requests approval there like any new Machine. Only the local
//! owner can start this (local socket), and only the new control's owner can
//! approve the workers.

use super::{proxy::worker_url, ControlService};
use crate::link_audit::LinkAuditRecord;
use protocol::{
    link::MachineRole,
    pairing::{LinkMoveRequest, WorkerHandoff},
    MachineId,
};
use store::registry::RegistryState;

impl ControlService {
    /// List (`dry_run`) or move every active worker to `target`.
    pub async fn handoff_workers(
        &self,
        target: &LinkMoveRequest,
        dry_run: bool,
    ) -> Result<Vec<WorkerHandoff>, String> {
        let policy = if self.settings.development {
            protocol::link::ControlUrlPolicy::Development
        } else {
            protocol::link::ControlUrlPolicy::Production
        };
        target.validate(policy).map_err(|error| error.to_string())?;
        if target.control_machine_id == self.settings.machine_id {
            return Err(
                "that is this control; choose the control the workers should move to".into(),
            );
        }
        let workers = self
            .store
            .registry_machines()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|row| row.role == MachineRole::Worker && row.state == RegistryState::Active);
        let mut results = Vec::new();
        for row in workers {
            let mut entry = WorkerHandoff {
                machine_id: row.machine_id.clone(),
                display_name: row.display_name.clone(),
                moved: false,
                message: None,
            };
            if !dry_run {
                match self.send_move(&row.machine_id, target).await {
                    Ok(()) => entry.moved = true,
                    Err(message) => entry.message = Some(message),
                }
                self.audit.append(&LinkAuditRecord {
                    machine_id: Some(&row.machine_id.0),
                    reason: Some(if entry.moved { "MOVED" } else { "NOT_MOVED" }),
                    ..LinkAuditRecord::new((self.clock)(), "WORKER_HANDOFF")
                });
            }
            results.push(entry);
        }
        Ok(results)
    }

    async fn send_move(
        &self,
        machine_id: &MachineId,
        target: &LinkMoveRequest,
    ) -> Result<(), String> {
        let routing = self
            .routing_target(machine_id)
            .map_err(|failure| failure.failure.message)?;
        let response = self
            .worker_http
            .post(worker_url(&routing, "http", "/api/v1/link/move"))
            .bearer_auth(routing.control_credential.expose())
            .json(target)
            .send()
            .await
            .map_err(|_| "the worker could not be reached over Tailscale".to_string())?;
        match response.status().as_u16() {
            202 => Ok(()),
            404 => Err("the worker runs a release without handoff (update it, then run sentinal setup on it)".into()),
            status => Err(format!("the worker refused the move (HTTP {status})")),
        }
    }
}
