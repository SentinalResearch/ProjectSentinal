//! Identity checks for approval pairing: the callback proof over the Tailnet
//! and `tailscale whois`, plus the verification words.

use protocol::{
    link::LinkEndpoint,
    pairing::{verification_transcript, PairingProof, TailscaleIdentity},
    MachineId,
};
use sha2::{Digest, Sha256};
use std::{future::Future, path::PathBuf, pin::Pin, time::Duration};

/// A boxed verification step.
pub type VerifyFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// The two identity checks, replaceable in tests.
pub trait PairingVerifier: Send + Sync {
    /// `GET http://<endpoint>/api/v1/pairing/proof/<request_id>` on the worker.
    fn proof<'a>(
        &'a self,
        endpoint: &'a LinkEndpoint,
        request_id: &'a str,
    ) -> VerifyFuture<'a, PairingProof>;
    /// `tailscale whois --json <address>`.
    fn whois<'a>(&'a self, address: &'a str) -> VerifyFuture<'a, TailscaleIdentity>;
}

/// The four words both sides show for one request.
pub fn verification_words(
    control_machine_id: &MachineId,
    request_id: &str,
    control_nonce: &str,
    request_secret_sha256: &str,
    worker_nonce: &str,
) -> Vec<String> {
    let transcript = verification_transcript(
        control_machine_id,
        request_id,
        control_nonce,
        request_secret_sha256,
        worker_nonce,
    );
    protocol::pairing::verification_words(&Sha256::digest(&transcript))
}

/// The real verifier: a bounded plain-HTTP callback over the Tailnet and the
/// local Tailscale CLI.
pub struct TailnetVerifier {
    http: reqwest::Client,
    tailscale: Option<PathBuf>,
}

impl Default for TailnetVerifier {
    fn default() -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let tailscale = crate::tailnet_dns::tailscale_cli();
        Self { http, tailscale }
    }
}

impl PairingVerifier for TailnetVerifier {
    fn proof<'a>(
        &'a self,
        endpoint: &'a LinkEndpoint,
        request_id: &'a str,
    ) -> VerifyFuture<'a, PairingProof> {
        Box::pin(async move {
            let host = match endpoint.address.parse::<std::net::IpAddr>() {
                Ok(std::net::IpAddr::V6(v6)) => format!("[{v6}]"),
                Ok(ip) => ip.to_string(),
                Err(_) => return Err("endpoint is not an IP address".into()),
            };
            let url = format!(
                "http://{host}:{}/api/v1/pairing/proof/{request_id}",
                endpoint.port
            );
            let response = self
                .http
                .get(url)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            if !response.status().is_success() {
                return Err(format!("status {}", response.status()));
            }
            let body = response.bytes().await.map_err(|error| error.to_string())?;
            if body.len() > 4096 {
                return Err("proof too large".into());
            }
            serde_json::from_slice(&body).map_err(|error| error.to_string())
        })
    }

    fn whois<'a>(&'a self, address: &'a str) -> VerifyFuture<'a, TailscaleIdentity> {
        Box::pin(async move {
            let cli = self.tailscale.as_ref().ok_or("Tailscale CLI not found")?;
            let output = tokio::time::timeout(
                Duration::from_secs(4),
                tokio::process::Command::new(cli)
                    .args(["whois", "--json", address])
                    .env("TAILSCALE_BE_CLI", "1")
                    .stdin(std::process::Stdio::null())
                    .output(),
            )
            .await
            .map_err(|_| "whois timed out".to_string())?
            .map_err(|error| error.to_string())?;
            if !output.status.success() || output.stdout.len() > 64 * 1024 {
                return Err("whois failed".into());
            }
            let value: serde_json::Value =
                serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
            let text = |pointer: &str| {
                value
                    .pointer(pointer)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            };
            let node = text("/Node/ComputedName")
                .or_else(|| {
                    text("/Node/Name")
                        .map(|name| name.split('.').next().unwrap_or_default().to_string())
                })
                .filter(|node| !node.is_empty())
                .ok_or("whois has no node name")?;
            Ok(TailscaleIdentity {
                node,
                login: text("/UserProfile/LoginName").unwrap_or_default(),
                os: text("/Node/Hostinfo/OS").unwrap_or_default(),
                address: address.to_string(),
            })
        })
    }
}
