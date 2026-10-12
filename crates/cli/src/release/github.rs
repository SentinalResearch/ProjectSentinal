//! Find the newest owner-signed release on GitHub for this control's channel.
//! Every network request goes through `curl` (HTTPS only, bounded).

use super::{channel_name, Context, ReleaseError, Version};
use crate::release_args::ReleaseChannel;
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

const MAX_API_BYTES: &str = "262144";
const MAX_SUMS_BYTES: &str = "65536";
const MAX_SIGNATURE_BYTES: &str = "8192";
const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

/// A release that verified against the pinned keys.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub(crate) version: Version,
    pub(crate) published_at: String,
    pub(crate) notes_url: String,
}

/// The result of one check.
#[derive(Debug, Clone)]
pub(crate) struct CheckOutcome {
    pub(crate) available: Option<Candidate>,
    /// The newest newer release that was not signed by a trusted key.
    pub(crate) unsigned_found: Option<Version>,
}

/// Check GitHub for a newer release on `channel`, signed by a pinned key.
pub(crate) fn check(
    context: &Context,
    channel: ReleaseChannel,
) -> Result<CheckOutcome, ReleaseError> {
    let list = context.scratch.path.join("releases.json");
    let url = format!(
        "https://api.github.com/repos/{}/releases?per_page=30",
        context.repo
    );
    if !curl_to_file(&url, &list, MAX_API_BYTES, false) {
        return Err(ReleaseError::Refused(format!(
            "could not reach GitHub releases for {}; the last check result was kept",
            context.repo
        )));
    }
    let text = std::fs::read_to_string(&list).unwrap_or_default();
    let releases: Vec<Value> = serde_json::from_str(&text)
        .map_err(|_| ReleaseError::Refused("GitHub returned an unexpected releases list".into()))?;

    let mut newer: Vec<(Version, &Value)> = releases
        .iter()
        .filter(|release| !release["draft"].as_bool().unwrap_or(true))
        .filter(|release| {
            channel == ReleaseChannel::Beta || !release["prerelease"].as_bool().unwrap_or(true)
        })
        .filter_map(|release| {
            let version = Version::parse(release["tag_name"].as_str()?.strip_prefix('v')?)?;
            (version > context.current).then_some((version, release))
        })
        .collect();
    newer.sort_by_key(|(version, _)| std::cmp::Reverse(*version));

    let mut unsigned_found = None;
    for (version, release) in newer {
        if signed_and_complete(context, version) {
            return Ok(CheckOutcome {
                available: Some(Candidate {
                    version,
                    published_at: release["published_at"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    notes_url: release["html_url"].as_str().unwrap_or_default().to_owned(),
                }),
                unsigned_found,
            });
        }
        unsigned_found.get_or_insert(version);
    }
    Ok(CheckOutcome {
        available: None,
        unsigned_found,
    })
}

/// Download the release's SHA256SUMS and signature and verify them.
fn signed_and_complete(context: &Context, version: Version) -> bool {
    let base = format!(
        "https://github.com/{}/releases/download/v{version}",
        context.repo
    );
    let sums = context.scratch.path.join(format!("{version}.SHA256SUMS"));
    let signature = context
        .scratch
        .path
        .join(format!("{version}.SHA256SUMS.sig"));
    if !curl_to_file(&format!("{base}/SHA256SUMS"), &sums, MAX_SUMS_BYTES, true)
        || !curl_to_file(
            &format!("{base}/SHA256SUMS.sig"),
            &signature,
            MAX_SIGNATURE_BYTES,
            true,
        )
        || !verify_signature(&context.allowed_signers, &sums, &signature)
    {
        return false;
    }
    let text = std::fs::read_to_string(&sums).unwrap_or_default();
    TARGETS.iter().all(|target| {
        let name = format!("sentinal-worker-{version}-{target}.tar.gz");
        text.lines().any(|line| {
            let mut fields = line.split("  ");
            fields.next().is_some_and(|digest| digest.len() == 64)
                && fields.next() == Some(name.as_str())
        })
    })
}

fn verify_signature(allowed: &Path, sums: &Path, signature: &Path) -> bool {
    let Ok(input) = std::fs::File::open(sums) else {
        return false;
    };
    Command::new("ssh-keygen")
        .args([
            "-Y",
            "verify",
            "-I",
            "sentinal-release",
            "-n",
            "sentinal-release",
            "-f",
        ])
        .arg(allowed)
        .arg("-s")
        .arg(signature)
        .stdin(input)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Fetch `url` into `output` over HTTPS (GitHub downloads redirect).
pub(crate) fn curl_to_file(url: &str, output: &Path, max_bytes: &str, follow: bool) -> bool {
    let mut command = Command::new("curl");
    command.args([
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--tlsv1.2",
        "--fail",
        "--silent",
        "--show-error",
    ]);
    if follow {
        command.args(["--location", "--max-redirs", "3"]);
    }
    command
        .args(["--max-filesize", max_bytes, "--max-time", "30", "--output"])
        .arg(output)
        .args([
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: sentinal-release",
        ])
        .arg(url)
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
        && std::fs::metadata(output).is_ok_and(|metadata| metadata.len() > 0)
}

/// Print one check result as text or JSON.
pub(crate) fn report(
    context: &Context,
    channel: ReleaseChannel,
    outcome: &CheckOutcome,
    as_json: bool,
    out: &mut dyn Write,
) {
    let channel = channel_name(channel);
    if as_json {
        let value = json!({
            "current": context.current.to_string(),
            "channel": channel,
            "available": outcome.available.as_ref().map(|candidate| json!({
                "version": candidate.version.to_string(),
                "signed": true,
                "published_at": candidate.published_at,
                "notes_url": candidate.notes_url,
            })),
            "unsigned_found": outcome.unsigned_found.map(|version| version.to_string()),
        });
        let _ = writeln!(out, "{value}");
        return;
    }
    if let Some(version) = outcome.unsigned_found {
        let _ = writeln!(
            out,
            "{version} found but not signed by a trusted key; it is not offered"
        );
    }
    match &outcome.available {
        Some(candidate) => {
            let _ = writeln!(
                out,
                "Control {} ({channel}) · {} available, signed by a trusted key",
                context.current, candidate.version
            );
            if !candidate.notes_url.is_empty() {
                let _ = writeln!(out, "  notes: {}", candidate.notes_url);
            }
            let _ = writeln!(out, "Run: sentinal release update");
        }
        None => {
            let _ = writeln!(out, "Control {} is current ({channel})", context.current);
        }
    }
}
