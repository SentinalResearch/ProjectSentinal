//! The control watching its links (milestone 4.H §1.3): delayed and offline
//! periods, link round trips, Tailscale paths, and route timings. Durations
//! and closed codes only.

use super::{projection_freshness, ControlService};
use protocol::{
    link::{MachineRole, RegistryStatus, TailnetPath},
    observability::{HealthLabel, HealthMetric, HealthSample, IncidentKind},
    MachineId, ProjectionFreshness, Timestamp,
};
use std::time::{Duration, Instant};

/// How often freshness is checked (delays shorter than this are not seen).
pub const FRESHNESS_EVERY: Duration = Duration::from_secs(10);
/// Link round-trip sampling (D3).
pub const LINK_RTT_EVERY_TICKS: u64 = 6;
/// Tailscale path sampling (D4).
pub const PATH_EVERY_TICKS: u64 = 30;

impl ControlService {
    /// Store one sample the control measured itself, if it is valid.
    pub fn record_health_sample(&self, sample: HealthSample) {
        if sample.validate((self.clock)()).is_err() {
            return;
        }
        if let Err(error) = self.store.record_health_samples(&[sample]) {
            tracing::warn!(%error, "failed to store a health sample");
        }
    }

    /// Time of one control route, attributed to `machine_id`.
    pub fn record_route(&self, machine_id: &MachineId, route: HealthLabel, elapsed: Duration) {
        self.record_health_sample(HealthSample {
            machine_id: machine_id.clone(),
            metric: HealthMetric::RouteMs,
            label: Some(route),
            at: (self.clock)(),
            value: elapsed.as_secs_f64() * 1_000.0,
        });
    }

    /// Every worker as `(id, registry status, freshness, sends projection)`.
    pub fn worker_freshness(&self) -> Vec<(MachineId, RegistryStatus, ProjectionFreshness, bool)> {
        let now = (self.clock)();
        let Ok(registry) = self.registry() else {
            return Vec::new();
        };
        registry
            .machines
            .into_iter()
            .filter(|machine| machine.role == MachineRole::Worker)
            .map(|machine| {
                let state = self
                    .store
                    .control_projection_state(&machine.machine_id)
                    .ok()
                    .flatten();
                let expired = machine
                    .credential
                    .as_ref()
                    .is_some_and(|credential| credential.expires_at.0 <= now.0);
                let freshness = projection_freshness(machine.status, expired, state.as_ref(), now);
                (
                    machine.machine_id,
                    machine.status,
                    freshness,
                    state.is_some(),
                )
            })
            .collect()
    }

    /// Open and close DELAYED and OFFLINE incidents from the current freshness.
    pub fn observe_freshness(&self) {
        let now = (self.clock)();
        for (machine_id, status, freshness, projected) in self.worker_freshness() {
            // An older worker without projection is never "delayed", only online.
            let (offline, delayed) = match (status, freshness) {
                (RegistryStatus::Offline, _) => (true, false),
                (RegistryStatus::Online, ProjectionFreshness::Stale) => (false, projected),
                _ => (false, false),
            };
            self.set_incident(&machine_id, IncidentKind::Offline, offline, now, None);
            self.set_incident(&machine_id, IncidentKind::Delayed, delayed, now, None);
        }
    }

    /// A worker's Tailscale path: a relay opens a RELAYED incident, a direct path closes it.
    pub fn record_tailnet_path(
        &self,
        machine_id: &MachineId,
        path: TailnetPath,
        relay: Option<&str>,
    ) {
        let now = (self.clock)();
        match path {
            TailnetPath::Relayed => {
                self.set_incident(machine_id, IncidentKind::Relayed, true, now, relay)
            }
            TailnetPath::Direct => {
                self.set_incident(machine_id, IncidentKind::Relayed, false, now, None)
            }
            TailnetPath::Unknown => {}
        }
    }

    fn set_incident(
        &self,
        machine_id: &MachineId,
        kind: IncidentKind,
        active: bool,
        at: Timestamp,
        detail: Option<&str>,
    ) {
        let result = if active {
            self.store
                .open_health_incident(machine_id, kind, at, detail)
                .map(|_| ())
        } else {
            self.store
                .close_health_incident(machine_id, kind, at)
                .map(|_| ())
        };
        if let Err(error) = result {
            tracing::warn!(%error, "failed to record a health incident");
        }
    }

    /// Workers whose link round trip is worth timing: online and connected.
    pub fn round_trip_targets(&self) -> Vec<MachineId> {
        self.worker_freshness()
            .into_iter()
            .filter(|(_, status, _, _)| *status == RegistryStatus::Online)
            .map(|(machine_id, _, _, _)| machine_id)
            .collect()
    }

    /// Time one identity round trip to each online worker.
    pub async fn sample_link_round_trips(&self) {
        for machine_id in self.round_trip_targets() {
            let Ok(target) = self.routing_target(&machine_id) else {
                continue;
            };
            let started = Instant::now();
            if super::proxy::verify_identity(&self.worker_http, &target)
                .await
                .is_ok()
            {
                self.record_health_sample(HealthSample {
                    machine_id,
                    metric: HealthMetric::LinkRttMs,
                    label: None,
                    at: (self.clock)(),
                    value: started.elapsed().as_secs_f64() * 1_000.0,
                });
            }
        }
    }

    /// Record every worker's Tailscale path from one `tailscale status`.
    pub async fn sample_tailnet_paths(&self) {
        let targets = self.round_trip_targets();
        if targets.is_empty() {
            return;
        }
        let Some(status) = tokio::task::spawn_blocking(crate::tailnet_dns::real_status)
            .await
            .ok()
            .flatten()
        else {
            return;
        };
        for machine_id in targets {
            if let Ok(target) = self.routing_target(&machine_id) {
                let (path, relay) = super::ping::peer_path(&status, &target.endpoint);
                self.record_tailnet_path(&machine_id, path, relay.as_deref());
            }
        }
    }
}

/// Run the monitor until shutdown.
pub fn spawn(
    control: ControlService,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(FRESHNESS_EVERY);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut tick: u64 = 0;
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = interval.tick() => {
                    control.observe_freshness();
                    if tick.is_multiple_of(LINK_RTT_EVERY_TICKS) {
                        control.sample_link_round_trips().await;
                    }
                    if tick.is_multiple_of(PATH_EVERY_TICKS) {
                        control.sample_tailnet_paths().await;
                    }
                    tick += 1;
                }
            }
        }
    })
}
