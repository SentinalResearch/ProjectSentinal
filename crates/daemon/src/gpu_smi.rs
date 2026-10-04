//! NVIDIA GPU sampling through `nvidia-smi`, used when the daemon is built
//! without NVML (release workers are static binaries that cannot load it).

use protocol::{Gpu, GpuMetrics};
use std::{
    collections::HashMap,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_millis(1_500);
const MIB: u64 = 1024 * 1024;

/// Utilisation and memory for every GPU, plus GPU memory per process ID.
/// Empty when `nvidia-smi` is missing, fails, or times out.
pub fn snapshot() -> (Vec<GpuMetrics>, HashMap<u32, u64>) {
    let Some(gpus) = run(&[
        "--query-gpu=index,name,utilization.gpu,memory.used,memory.total,temperature.gpu",
        "--format=csv,noheader,nounits",
    ]) else {
        return (Vec::new(), HashMap::new());
    };
    let apps = run(&[
        "--query-compute-apps=pid,used_memory",
        "--format=csv,noheader,nounits",
    ])
    .map(|text| parse_apps(&text))
    .unwrap_or_default();
    (
        parse_gpus(&gpus)
            .into_iter()
            .map(|(metrics, _)| metrics)
            .collect(),
        apps,
    )
}

/// Each GPU's name and total memory, for the static Machine description.
pub fn inventory() -> Vec<Gpu> {
    run(&[
        "--query-gpu=index,name,utilization.gpu,memory.used,memory.total,temperature.gpu",
        "--format=csv,noheader,nounits",
    ])
    .map(|text| {
        parse_gpus(&text)
            .into_iter()
            .map(|(metrics, name)| Gpu {
                name,
                vram_bytes: Some(metrics.vram_total_bytes),
            })
            .collect()
    })
    .unwrap_or_default()
}

fn run(args: &[&str]) -> Option<String> {
    let mut child = Command::new("nvidia-smi")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    let output = child.wait_with_output().ok()?;
    String::from_utf8(output.stdout).ok()
}

/// `index, name, util %, used MiB, total MiB, temp °C` rows.
fn parse_gpus(text: &str) -> Vec<(GpuMetrics, String)> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').map(str::trim).collect();
            let [index, name, util, used, total, temp] = fields.as_slice() else {
                return None;
            };
            Some((
                GpuMetrics {
                    index: index.parse().ok()?,
                    util_percent: util.parse().unwrap_or(0.0),
                    vram_used_bytes: used.parse::<u64>().ok()?.saturating_mul(MIB),
                    vram_total_bytes: total.parse::<u64>().ok()?.saturating_mul(MIB),
                    temp_c: temp.parse().ok(),
                },
                (*name).to_owned(),
            ))
        })
        .collect()
}

/// `pid, used MiB` rows; a process on several GPUs is summed.
fn parse_apps(text: &str) -> HashMap<u32, u64> {
    let mut by_pid = HashMap::new();
    for line in text.lines() {
        let mut fields = line.split(',').map(str::trim);
        let (Some(pid), Some(used)) = (fields.next(), fields.next()) else {
            continue;
        };
        let (Ok(pid), Ok(used)) = (pid.parse::<u32>(), used.parse::<u64>()) else {
            continue;
        };
        *by_pid.entry(pid).or_insert(0_u64) += used.saturating_mul(MIB);
    }
    by_pid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_gpu_row_and_skips_unavailable_values() {
        let gpus = parse_gpus(
            "0, NVIDIA GeForce RTX 5060 Ti, 95, 21914, 24576, 71\n\
             1, NVIDIA GeForce RTX 5060 Ti, [N/A], 151, 8151, [N/A]\n\
             garbage line\n",
        );
        assert_eq!(gpus.len(), 2);
        let (first, name) = &gpus[0];
        assert_eq!(name, "NVIDIA GeForce RTX 5060 Ti");
        assert_eq!(first.index, 0);
        assert_eq!(first.util_percent, 95.0);
        assert_eq!(first.vram_used_bytes, 21_914 * MIB);
        assert_eq!(first.vram_total_bytes, 24_576 * MIB);
        assert_eq!(first.temp_c, Some(71.0));
        assert_eq!(gpus[1].0.util_percent, 0.0);
        assert_eq!(gpus[1].0.temp_c, None);
    }

    #[test]
    fn sums_a_process_across_gpus() {
        let apps = parse_apps("2594, 128\n2594, 128\n77, 1024\nbad\n");
        assert_eq!(apps.get(&2594), Some(&(256 * MIB)));
        assert_eq!(apps.get(&77), Some(&(1024 * MIB)));
        assert_eq!(apps.len(), 2);
    }
}
