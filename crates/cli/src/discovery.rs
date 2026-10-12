//! Find Sentinal controls on the Tailnet (4.E §4). Candidates come only from
//! `tailscale status --json`: online peers with a `.ts.net` name and a
//! Tailscale address. No CIDR scan, mDNS, or LAN address is ever tried. Each
//! probe is pinned to the peer's Tailscale address, so DNS cannot redirect it,
//! and an answer counts only when it names exactly the probed origin.

use protocol::pairing::DiscoveryResponse;
use serde_json::Value;
use std::{
    net::IpAddr,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

/// Peers probed at most.
pub const MAX_PEERS: usize = 64;
/// Probes in flight at most.
pub const MAX_CONCURRENT: usize = 8;
/// Largest discovery body accepted.
pub const MAX_BODY_BYTES: usize = 4096;
/// Whole-run deadline used by `sentinal setup`.
pub const DEADLINE: Duration = Duration::from_secs(6);

/// One probe's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// HTTP 200 with this body.
    Body(Vec<u8>),
    /// HTTP 404: a Sentinal daemon older than discovery (or not Sentinal).
    NotFound,
    /// Timeout, refusal, TLS failure, redirect, or any other status.
    NoAnswer,
}

/// Tailscale status and HTTPS probes.
pub trait Transport: Sync {
    /// `tailscale status --json`.
    fn status(&self) -> Result<Value, String>;
    /// `GET https://<host>/api/v1/discovery`, connecting only to `ip`.
    fn probe(&self, host: &str, ip: IpAddr) -> Probe;
}

/// A control that answered for its own origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundControl {
    /// Its validated answer.
    pub discovery: DiscoveryResponse,
    /// The Tailscale address it answered on.
    pub address: IpAddr,
}

/// Everything one discovery run learned.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// Valid controls, by name then origin.
    pub controls: Vec<FoundControl>,
    /// Origins that answered 404 (older Sentinal: use URL + code).
    pub older: Vec<String>,
    /// Peers probed.
    pub checked: usize,
    /// Peers that did not give a usable answer.
    pub unanswered: usize,
}

/// Probe the Tailnet within `deadline`.
pub fn discover(transport: &dyn Transport, deadline: Duration) -> Result<Discovered, String> {
    let status = transport.status()?;
    if status.get("BackendState").and_then(Value::as_str) != Some("Running") {
        return Err("Tailscale is not connected: run 'tailscale up', then sentinal setup".into());
    }
    let mut candidates: Vec<(String, IpAddr)> = status
        .get("Peer")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|peers| peers.values())
        .filter_map(candidate)
        .collect();
    candidates.sort();
    candidates.dedup_by(|a, b| a.0 == b.0);
    candidates.truncate(MAX_PEERS);

    let stop_at = Instant::now() + deadline;
    let queue = Mutex::new(candidates.into_iter());
    let results = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..MAX_CONCURRENT {
            scope.spawn(|| loop {
                if Instant::now() >= stop_at {
                    return;
                }
                let next = queue.lock().ok().and_then(|mut queue| queue.next());
                let Some((host, ip)) = next else {
                    return;
                };
                let probe = transport.probe(&host, ip);
                if let Ok(mut results) = results.lock() {
                    results.push((host, ip, probe));
                }
            });
        }
    });

    let mut found = Discovered::default();
    for (host, address, probe) in results.into_inner().unwrap_or_default() {
        found.checked += 1;
        let origin = format!("https://{host}");
        match probe {
            Probe::Body(body) if body.len() <= MAX_BODY_BYTES => {
                match serde_json::from_slice::<DiscoveryResponse>(&body) {
                    Ok(discovery) if discovery.validate(&origin).is_ok() => {
                        found.controls.push(FoundControl { discovery, address })
                    }
                    _ => found.unanswered += 1,
                }
            }
            Probe::NotFound => found.older.push(origin),
            _ => found.unanswered += 1,
        }
    }
    found.controls.sort_by(|a, b| {
        (&a.discovery.display_name, &a.discovery.origin)
            .cmp(&(&b.discovery.display_name, &b.discovery.origin))
    });
    found.older.sort();
    Ok(found)
}

/// An online peer with a `.ts.net` name and a Tailscale address.
fn candidate(peer: &Value) -> Option<(String, IpAddr)> {
    if peer.get("Online").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let host = peer
        .get("DNSName")?
        .as_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let safe = host.ends_with(".ts.net")
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
    if !safe {
        return None;
    }
    let ip = peer
        .get("TailscaleIPs")?
        .as_array()?
        .iter()
        .filter_map(|ip| ip.as_str()?.parse::<IpAddr>().ok())
        .find(is_tailscale)?;
    Some((host, ip))
}

fn is_tailscale(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => u32::from(*v4) & 0xffc0_0000 == 0x6440_0000,
        IpAddr::V6(v6) => v6.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// The real Tailnet: the Tailscale CLI and `curl`.
pub struct RealTransport {
    /// Tailscale CLI path.
    pub tailscale: PathBuf,
}

impl Transport for RealTransport {
    fn status(&self) -> Result<Value, String> {
        let output = Command::new(&self.tailscale)
            .args(["status", "--json"])
            .env("TAILSCALE_BE_CLI", "1")
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("could not run tailscale: {error}"))?;
        if !output.status.success() || output.stdout.len() > 8 * 1024 * 1024 {
            return Err("tailscale status failed".into());
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("tailscale status was not JSON: {error}"))
    }

    fn probe(&self, host: &str, ip: IpAddr) -> Probe {
        let address = match ip {
            IpAddr::V4(v4) => v4.to_string(),
            IpAddr::V6(v6) => format!("[{v6}]"),
        };
        let output = Command::new("curl")
            .args([
                "--silent",
                "--proto",
                "=https",
                "--max-redirs",
                "0",
                "--connect-timeout",
                "2",
                "--max-time",
                "2",
                "--max-filesize",
                &MAX_BODY_BYTES.to_string(),
                "--resolve",
                &format!("{host}:443:{address}"),
                "--write-out",
                "\n%{http_code}",
            ])
            .arg(format!("https://{host}/api/v1/discovery"))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let Ok(output) = output else {
            return Probe::NoAnswer;
        };
        let text = output.stdout;
        let Some(split) = text.iter().rposition(|byte| *byte == b'\n') else {
            return Probe::NoAnswer;
        };
        match &text[split + 1..] {
            b"200" => Probe::Body(text[..split].to_vec()),
            b"404" => Probe::NotFound,
            _ => Probe::NoAnswer,
        }
    }
}
