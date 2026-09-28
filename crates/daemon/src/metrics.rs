//! System resource sampling and high-resource debounce logic.

use protocol::{
    tree_totals, GpuMetrics, MachineMetrics, Process, ProcessState, Resource, SessionId,
    SessionMetrics, Timestamp,
};
use std::collections::HashMap;
use sysinfo::{ProcessStatus, System};

const HIGH_PERCENT: f32 = 90.0;
const HIGH_SAMPLES: u8 = 3;

/// Machine, process, and per-Session values from one sampling instant.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsSnapshot {
    /// Whole-machine measurements.
    pub machine: MachineMetrics,
    /// Processes visible to the sampler.
    pub processes: Vec<Process>,
    /// Totals for each supplied Session root PID.
    pub sessions: Vec<SessionMetrics>,
}

/// Stateful `sysinfo` sampler; call it at the configured interval.
pub struct MetricsSampler {
    system: System,
}

impl Default for MetricsSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsSampler {
    /// Create a sampler primed with an initial system snapshot.
    pub fn new() -> Self {
        MetricsSampler {
            system: System::new_all(),
        }
    }

    /// Refresh the host and aggregate process trees for Session root PIDs.
    pub fn sample(&mut self, at: Timestamp, roots: &[(SessionId, u32)]) -> MetricsSnapshot {
        self.system.refresh_all();
        let (gpus, gpu_by_pid) = nvidia_snapshot();
        let processes = self
            .system
            .processes()
            .values()
            .map(|process| Process {
                pid: process.pid().as_u32(),
                parent_pid: process.parent().map(|pid| pid.as_u32()),
                name: process.name().to_string_lossy().into_owned(),
                cmdline: process
                    .cmd()
                    .iter()
                    .map(|part| part.to_string_lossy().into_owned())
                    .collect(),
                state: process_state(process.status()),
                cpu_percent: process.cpu_usage(),
                rss_bytes: process.memory(),
                gpu_mem_bytes: gpu_by_pid.get(&process.pid().as_u32()).copied(),
                started_at: Timestamp(
                    i64::try_from(process.start_time())
                        .unwrap_or(i64::MAX)
                        .saturating_mul(1_000),
                ),
            })
            .collect::<Vec<_>>();
        let sessions = roots
            .iter()
            .map(|(session_id, root)| SessionMetrics {
                session_id: *session_id,
                at,
                totals: tree_totals(&processes, *root),
            })
            .collect();
        MetricsSnapshot {
            machine: MachineMetrics {
                at,
                cpu_percent: self.system.global_cpu_usage(),
                ram_used_bytes: self.system.used_memory(),
                ram_total_bytes: self.system.total_memory(),
                gpus,
            },
            processes,
            sessions,
        }
    }
}

/// One newly crossed resource threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResourceCrossing {
    /// Resource that stayed high.
    pub resource: Resource,
    /// Latest sampled percentage.
    pub percent: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct DebounceState {
    consecutive: u8,
    emitted: bool,
}

/// Emits once when a resource is at least 90% for three consecutive samples.
#[derive(Debug, Default)]
pub struct ResourceDebouncer {
    states: HashMap<Resource, DebounceState>,
}

impl ResourceDebouncer {
    /// Create a debouncer with no active crossings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe one sample containing zero or more resource percentages.
    pub fn observe(&mut self, values: &[(Resource, f32)]) -> Vec<ResourceCrossing> {
        let mut maxima = HashMap::<Resource, f32>::new();
        for (resource, percent) in values {
            maxima
                .entry(*resource)
                .and_modify(|value| *value = value.max(*percent))
                .or_insert(*percent);
        }
        for (resource, state) in &mut self.states {
            if !maxima.contains_key(resource) {
                *state = DebounceState::default();
            }
        }
        let mut crossings = Vec::new();
        for (resource, percent) in maxima {
            let state = self.states.entry(resource).or_default();
            if percent >= HIGH_PERCENT {
                state.consecutive = state.consecutive.saturating_add(1);
                if state.consecutive >= HIGH_SAMPLES && !state.emitted {
                    state.emitted = true;
                    crossings.push(ResourceCrossing { resource, percent });
                }
            } else {
                *state = DebounceState::default();
            }
        }
        crossings
    }
}

fn process_state(status: ProcessStatus) -> ProcessState {
    match status {
        ProcessStatus::Run => ProcessState::Running,
        ProcessStatus::Idle | ProcessStatus::Sleep | ProcessStatus::UninterruptibleDiskSleep => {
            ProcessState::Sleeping
        }
        ProcessStatus::Stop | ProcessStatus::Tracing | ProcessStatus::Parked => {
            ProcessState::Stopped
        }
        ProcessStatus::Zombie | ProcessStatus::Dead => ProcessState::Zombie,
        _ => ProcessState::Unknown,
    }
}

#[cfg(not(feature = "nvidia"))]
fn nvidia_snapshot() -> (Vec<GpuMetrics>, HashMap<u32, u64>) {
    (Vec::new(), HashMap::new())
}

#[cfg(feature = "nvidia")]
fn nvidia_snapshot() -> (Vec<GpuMetrics>, HashMap<u32, u64>) {
    use nvml_wrapper::{
        enum_wrappers::device::TemperatureSensor, enums::device::UsedGpuMemory, Nvml,
    };

    let Ok(nvml) = Nvml::init() else {
        return (Vec::new(), HashMap::new());
    };
    let Ok(count) = nvml.device_count() else {
        return (Vec::new(), HashMap::new());
    };
    let mut metrics = Vec::new();
    let mut gpu_by_pid = HashMap::<u32, u64>::new();
    for index in 0..count {
        let Ok(device) = nvml.device_by_index(index) else {
            continue;
        };
        let (Ok(utilization), Ok(memory)) = (device.utilization_rates(), device.memory_info())
        else {
            continue;
        };
        metrics.push(GpuMetrics {
            index,
            util_percent: utilization.gpu as f32,
            vram_used_bytes: memory.used,
            vram_total_bytes: memory.total,
            temp_c: device
                .temperature(TemperatureSensor::Gpu)
                .ok()
                .map(|value| value as f32),
        });
        let mut device_by_pid = HashMap::<u32, u64>::new();
        for process in device
            .running_compute_processes()
            .unwrap_or_default()
            .into_iter()
            .chain(device.running_graphics_processes().unwrap_or_default())
        {
            if let UsedGpuMemory::Used(bytes) = process.used_gpu_memory {
                device_by_pid
                    .entry(process.pid)
                    .and_modify(|current| *current = (*current).max(bytes))
                    .or_insert(bytes);
            }
        }
        for (pid, bytes) in device_by_pid {
            gpu_by_pid
                .entry(pid)
                .and_modify(|current| *current = current.saturating_add(bytes))
                .or_insert(bytes);
        }
    }
    (metrics, gpu_by_pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_resource_emits_on_third_sample_only() {
        let mut debounce = ResourceDebouncer::new();
        let sample = [(Resource::Gpu, 96.0)];
        assert!(debounce.observe(&sample).is_empty());
        assert!(debounce.observe(&sample).is_empty());
        assert_eq!(
            debounce.observe(&sample),
            vec![ResourceCrossing {
                resource: Resource::Gpu,
                percent: 96.0
            }]
        );
        assert!(debounce.observe(&sample).is_empty());
    }

    #[test]
    fn crossing_rearms_after_dropping_below_threshold() {
        let mut debounce = ResourceDebouncer::new();
        for _ in 0..3 {
            debounce.observe(&[(Resource::Ram, 90.0)]);
        }
        assert!(debounce.observe(&[(Resource::Ram, 89.9)]).is_empty());
        assert!(debounce.observe(&[(Resource::Ram, 95.0)]).is_empty());
        assert!(debounce.observe(&[(Resource::Ram, 95.0)]).is_empty());
        assert_eq!(debounce.observe(&[(Resource::Ram, 95.0)]).len(), 1);
    }

    #[test]
    fn highest_gpu_value_counts_once_per_sample() {
        let mut debounce = ResourceDebouncer::new();
        let values = [(Resource::Gpu, 20.0), (Resource::Gpu, 99.0)];
        assert!(debounce.observe(&values).is_empty());
        assert!(debounce.observe(&values).is_empty());
        assert_eq!(debounce.observe(&values)[0].percent, 99.0);
    }

    #[test]
    fn missing_resource_rearms_the_next_crossing() {
        let mut debounce = ResourceDebouncer::new();
        for _ in 0..3 {
            debounce.observe(&[(Resource::Vram, 95.0)]);
        }
        assert!(debounce.observe(&[]).is_empty());
        assert!(debounce.observe(&[(Resource::Vram, 96.0)]).is_empty());
        assert!(debounce.observe(&[(Resource::Vram, 96.0)]).is_empty());
        assert_eq!(debounce.observe(&[(Resource::Vram, 96.0)]).len(), 1);
    }
}
