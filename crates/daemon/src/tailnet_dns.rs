//! Name resolution for links to other Machines. `*.ts.net` names resolve to
//! the peer's Tailscale address from `tailscale status --json`, the same
//! source discovery trusts, so a link works even where `/etc/resolv.conf`
//! does not reach MagicDNS. The static (musl) daemon reads that file
//! directly; e.g. Google Cloud's Ubuntu images point it at the metadata
//! DNS, which knows no Tailnet names. Every other name uses normal DNS.

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde_json::Value;
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// How long one `tailscale status` answer is reused.
const CACHE_FOR: Duration = Duration::from_secs(30);

/// Reads `tailscale status --json`; replaceable in tests.
pub type StatusSource = Arc<dyn Fn() -> Option<Value> + Send + Sync>;

/// The Tailscale CLI: `SENTINAL_TAILSCALE_CLI`, `tailscale` on PATH, or the macOS app.
pub fn tailscale_cli() -> Option<PathBuf> {
    std::env::var_os("SENTINAL_TAILSCALE_CLI")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join("tailscale"))
                    .find(|path| path.is_file())
            })
        })
        .or_else(|| {
            let app = PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
            app.is_file().then_some(app)
        })
}

/// `tailscale status --json` from the real CLI (bounded).
pub fn real_status() -> Option<Value> {
    let output = std::process::Command::new(tailscale_cli()?)
        .args(["status", "--json"])
        .env("TAILSCALE_BE_CLI", "1")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 8 * 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// The Tailscale addresses of the node named `host` (Self or a peer), IPv4 first.
pub fn peer_addresses(status: &Value, host: &str) -> Vec<IpAddr> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let nodes = status.get("Self").into_iter().chain(
        status
            .get("Peer")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|peers| peers.values()),
    );
    for node in nodes {
        let name = node
            .get("DNSName")
            .and_then(Value::as_str)
            .map(|name| name.trim_end_matches('.').to_ascii_lowercase());
        if name.as_deref() != Some(host.as_str()) {
            continue;
        }
        let mut addresses: Vec<IpAddr> = node
            .get("TailscaleIPs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|ip| ip.as_str()?.parse().ok())
            .filter(is_tailscale)
            .collect();
        addresses.sort_by_key(|ip| !ip.is_ipv4());
        return addresses;
    }
    Vec::new()
}

fn is_tailscale(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => u32::from(*v4) & 0xffc0_0000 == 0x6440_0000,
        IpAddr::V6(v6) => v6.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// A reqwest resolver: Tailnet names from Tailscale, the rest from DNS.
pub struct TailnetResolver {
    status: StatusSource,
    cache: Mutex<Option<(Instant, Arc<Value>)>>,
}

impl TailnetResolver {
    /// Resolve with `status` as the Tailscale source.
    pub fn new(status: StatusSource) -> Arc<Self> {
        Arc::new(Self {
            status,
            cache: Mutex::new(None),
        })
    }

    fn status(&self) -> Option<Arc<Value>> {
        if let Ok(cache) = self.cache.lock() {
            if let Some((_, value)) = cache.as_ref().filter(|(at, _)| at.elapsed() < CACHE_FOR) {
                return Some(value.clone());
            }
        }
        let value = Arc::new((self.status)()?);
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some((Instant::now(), value.clone()));
        }
        Some(value)
    }
}

impl Resolve for TailnetResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let tailnet = host
            .trim_end_matches('.')
            .to_ascii_lowercase()
            .ends_with(".ts.net");
        let found = if tailnet {
            self.status()
                .map(|status| peer_addresses(&status, &host))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Box::pin(async move {
            if !found.is_empty() {
                let addrs: Addrs = Box::new(found.into_iter().map(|ip| SocketAddr::new(ip, 0)));
                return Ok(addrs);
            }
            let resolved: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let addrs: Addrs = Box::new(resolved.into_iter());
            Ok(addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{peer_addresses, TailnetResolver};
    use reqwest::dns::{Name, Resolve};
    use serde_json::json;
    use std::{
        str::FromStr,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    fn status() -> serde_json::Value {
        json!({
            "Self": {"DNSName": "worker.tailnet.ts.net.", "TailscaleIPs": ["100.88.176.80"]},
            "Peer": {
                "nodekey:a": {"DNSName": "Control-One.tailnet.ts.net.", "TailscaleIPs": ["fd7a:115c:a1e0::5", "100.124.0.101"]},
                "nodekey:b": {"DNSName": "lan.tailnet.ts.net.", "TailscaleIPs": ["192.168.1.5"]}
            }
        })
    }

    #[test]
    fn tailnet_names_map_to_tailscale_addresses_only() {
        let status = status();
        let control = peer_addresses(&status, "control-one.tailnet.ts.net");
        assert_eq!(control[0].to_string(), "100.124.0.101", "IPv4 first");
        assert_eq!(control.len(), 2);
        assert_eq!(
            peer_addresses(&status, "worker.tailnet.ts.net.")[0].to_string(),
            "100.88.176.80"
        );
        assert!(
            peer_addresses(&status, "lan.tailnet.ts.net").is_empty(),
            "never a LAN address"
        );
        assert!(peer_addresses(&status, "unknown.tailnet.ts.net").is_empty());
    }

    #[tokio::test]
    async fn the_resolver_uses_tailscale_for_ts_net_and_dns_otherwise() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let resolver = TailnetResolver::new(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Some(status())
        }));
        for _ in 0..3 {
            let addrs: Vec<_> = resolver
                .resolve(Name::from_str("control-one.tailnet.ts.net").unwrap())
                .await
                .unwrap()
                .collect();
            assert_eq!(addrs[0].ip().to_string(), "100.124.0.101");
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "tailscale status is cached"
        );
        let local: Vec<_> = resolver
            .resolve(Name::from_str("localhost").unwrap())
            .await
            .unwrap()
            .collect();
        assert!(
            local.iter().any(|addr| addr.ip().is_loopback()),
            "{local:?}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "other names never ask Tailscale"
        );
    }
}
