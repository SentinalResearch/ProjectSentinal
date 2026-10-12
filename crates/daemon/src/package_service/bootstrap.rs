//! Generated, bounded worker bootstrap script.

use super::manifest::{ReleaseManifest, TargetArch, TargetOs};

/// Generate a bootstrap script from an already validated manifest.
pub(super) fn generate(base_url: &str, manifest: &ReleaseManifest, release_keys: &str) -> String {
    // Only well-formed key lines reach the heredoc, so the terminator cannot appear.
    let release_keys: String = super::release_keys::key_lines(release_keys)
        .into_iter()
        .map(|line| format!("{line}\n"))
        .collect();
    let mut cases = String::new();
    for target in &manifest.targets {
        let system = match target.os {
            TargetOs::Linux => "Linux",
            TargetOs::Macos => "Darwin",
        };
        let architectures = match target.arch {
            TargetArch::X86_64 => "x86_64|amd64",
            TargetArch::Aarch64 => "arm64|aarch64",
        };
        cases.push_str(&format!(
            "  {system}:{architectures}) artifact_path='{}'; expected_bytes='{}'; expected_sha='{}' ;;\n",
            target.artifact_path, target.bytes, target.sha256
        ));
    }
    format!(
        r#"#!/bin/sh
set -eu

fail() {{
  printf '%s\n' "sentinal bootstrap: $*" >&2
  exit 1
}}

control_url='{base_url}'
release_version='{}'
mode='install'
rollback=''
prefix="${{HOME:-}}/.local"
prefix_explicit='false'
while [ "$#" -gt 0 ]; do
  case "$1" in
    --check) mode='check'; shift ;;
    --rollback) [ "$#" -ge 2 ] || fail "--rollback requires a version"; rollback="$2"; shift 2 ;;
    --prefix) [ "$#" -ge 2 ] || fail "--prefix requires a path"; prefix="$2"; prefix_explicit='true'; shift 2 ;;
    *) fail "unknown bootstrap argument '$1'" ;;
  esac
done
[ -z "$rollback" ] || [ "$rollback" = "$release_version" ] || fail "requested rollback '$rollback' is not the published release '$release_version'"
if [ "$mode" = check ]; then
  state="$prefix/share/sentinal/release.json"
  if [ ! -f "$state" ]; then
    printf 'Sentinal is not installed at %s; stable is %s\n' "$prefix" "$release_version"
    exit 0
  fi
  installed="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$state" | head -n 1)"
  [ -n "$installed" ] || fail "installed release metadata has no version"
  if [ "$installed" = "$release_version" ]; then
    printf 'Sentinal %s is current\n' "$installed"
  else
    printf 'Sentinal installed: %s; stable: %s\n' "$installed" "$release_version"
  fi
  exit 0
fi
platform="$(uname -s):$(uname -m)"
case "$platform" in
{cases}  *) fail "unsupported target '$platform'" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"
tailscale_cli="${{SENTINAL_TAILSCALE_CLI:-}}"
if [ -n "$tailscale_cli" ]; then
  [ -x "$tailscale_cli" ] || fail "Tailscale CLI '$tailscale_cli' is not executable"
elif tailscale_cli="$(command -v tailscale 2>/dev/null)"; then
  :
elif [ "$(uname -s)" = 'Darwin' ] && [ -x '/Applications/Tailscale.app/Contents/MacOS/Tailscale' ]; then
  tailscale_cli='/Applications/Tailscale.app/Contents/MacOS/Tailscale'
else
  fail "Tailscale CLI was not found; install it or enable the macOS app CLI integration"
fi

state="$prefix/share/sentinal/release.json"
installed=''
[ ! -f "$state" ] || installed="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$state" | head -n 1)"
if [ -n "$installed" ]; then
  printf 'Sentinal %s → %s (%s)\n' "$installed" "$release_version" "$platform"
else
  printf 'Installing Sentinal %s (%s)\n' "$release_version" "$platform"
fi

temporary="$(mktemp -d "${{TMPDIR:-/tmp}}/sentinal-bootstrap.XXXXXX")" || fail "cannot create temporary directory"
trap 'rm -rf -- "$temporary"' EXIT HUP INT TERM
archive="$temporary/worker.tar.gz"

# Trust only releases the owner signed with a pinned key, checked with OpenSSH
# independently of this control's HTTPS origin.
release_keys="$temporary/allowed_signers"
cat >"$release_keys" <<'SENTINAL_RELEASE_KEYS'
{release_keys}SENTINAL_RELEASE_KEYS
grep -q '^sentinal-release namespaces="sentinal-release" ' "$release_keys" ||
  fail "this control has no pinned release signing keys; nothing was installed"
command -v ssh-keygen >/dev/null 2>&1 || fail "ssh-keygen (OpenSSH 8.2 or newer) is required to verify the release signature"
sums="$temporary/SHA256SUMS"
curl --proto '=https' --proto-redir '=https' --max-redirs 0 --fail --silent --show-error --max-filesize 65536 \
  --output "$sums" "$control_url/packages/v1/releases/$release_version.SHA256SUMS" ||
  fail "release $release_version is not signed on this control (SHA256SUMS missing); nothing was installed"
curl --proto '=https' --proto-redir '=https' --max-redirs 0 --fail --silent --show-error --max-filesize 8192 \
  --output "$sums.sig" "$control_url/packages/v1/releases/$release_version.SHA256SUMS.sig" ||
  fail "release $release_version is not signed on this control (signature missing); nothing was installed"
ssh-keygen -Y verify -f "$release_keys" -I sentinal-release -n sentinal-release -s "$sums.sig" <"$sums" >/dev/null 2>&1 ||
  fail "release $release_version is not signed by a trusted Sentinal release key; nothing was installed"
signed_sha="$(awk -v name="${{artifact_path##*/}}" '$2 == name {{ print $1 }}' "$sums")"
[ -n "$signed_sha" ] && [ "$signed_sha" = "$expected_sha" ] ||
  fail "the control's manifest does not match the signed SHA256SUMS; nothing was installed"

size_mb="$(awk -v bytes="$expected_bytes" 'BEGIN {{ printf "%.1f", bytes / 1048576 }}')"
printf '[1/4] Downloading %s MB\n' "$size_mb"
if [ -t 2 ]; then
  progress='--progress-bar'
else
  progress='--silent'
fi
curl --proto '=https' --proto-redir '=https' --max-redirs 0 --fail "$progress" --show-error \
  --output "$archive" "$control_url$artifact_path" || fail "artifact download failed"
actual_bytes="$(wc -c <"$archive" | tr -d '[:space:]')"
[ "$actual_bytes" = "$expected_bytes" ] || fail "artifact length mismatch: expected $expected_bytes, received $actual_bytes"

if command -v sha256sum >/dev/null 2>&1; then
  actual_sha="$(sha256sum "$archive" | awk '{{print $1}}')"
elif command -v shasum >/dev/null 2>&1; then
  actual_sha="$(shasum -a 256 "$archive" | awk '{{print $1}}')"
else
  fail "sha256sum or shasum is required"
fi
[ "$actual_sha" = "$expected_sha" ] || fail "SHA-256 mismatch; nothing was installed"

tar -tzf "$archive" >"$temporary/entries" || fail "artifact is not a readable gzip archive"
while IFS= read -r entry; do
  case "$entry" in
    sentinal-worker|sentinal-worker/*) ;;
    *) fail "archive contains unsafe entry '$entry'" ;;
  esac
  case "/$entry/" in
    */../*|*/./*) fail "archive contains unsafe entry '$entry'" ;;
  esac
done <"$temporary/entries"
tar -tvzf "$archive" | while IFS= read -r entry; do
  case "$entry" in
    -*) ;;
    d*) ;;
    *) fail "archive contains a non-file entry" ;;
  esac
done

mkdir "$temporary/extracted"
tar -xzf "$archive" -C "$temporary/extracted" || fail "artifact extraction failed"
release="$temporary/extracted/sentinal-worker"
[ -x "$release/bin/sentinal" ] || fail "artifact is missing bin/sentinal"
[ -x "$release/bin/sentinald" ] || fail "artifact is missing bin/sentinald"
[ -f "$release/scripts/install-worker.sh" ] || fail "artifact is missing the installer"
printf '[2/4] Verifying   owner signature, size, SHA-256, and archive entries ✓\n'

bind_ip="$(TAILSCALE_BE_CLI=1 "$tailscale_cli" ip -4 2>/dev/null | awk 'NR == 1 {{ print; exit }}')"
case "$bind_ip" in
  100.*) ;;
  *) fail "Tailscale is not connected; no Tailnet IPv4 address was returned" ;;
esac
second="$(printf '%s' "$bind_ip" | awk -F. '{{print $2}}')"
[ "$second" -ge 64 ] 2>/dev/null && [ "$second" -le 127 ] 2>/dev/null || fail "Tailscale address '$bind_ip' is outside 100.64.0.0/10"

set --
[ -z "$rollback" ] || set -- "$@" --rollback "$rollback"
[ "$prefix_explicit" = 'false' ] || set -- "$@" --prefix "$prefix"
printf '[3/4] Installing  programs, shell hooks, web app; restarting sentinald\n'
if ! "$release/scripts/install-worker.sh" install --release-dir "$release" --bind "$bind_ip" \
  --control-url "$control_url" --artifact-sha "$expected_sha" "$@" >"$temporary/install.log" 2>&1; then
  cat "$temporary/install.log" >&2
  fail "installation failed; the previous programs and all data were kept"
fi
[ -z "$installed" ] || printf '      Live FULL Sessions on this Machine were interrupted by the restart (until Reliability R1).\n'

# The restarted daemon needs a moment before it answers; read its link state.
link_state=''
link_control=''
attempt=0
while [ "$attempt" -lt 15 ]; do
  link_json="$("$release/bin/sentinal" connection status --json 2>/dev/null || true)"
  link_state="$(printf '%s\n' "$link_json" | sed -n 's/.*"state"[[:space:]]*:[[:space:]]*"\([A-Z_]*\)".*/\1/p' | head -n 1)"
  link_control="$(printf '%s\n' "$link_json" | sed -n 's/.*"control_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -z "$link_state" ] || break
  attempt=$((attempt + 1))
  sleep 1
done
connect_hint() {{
  printf '%s\n' 'Next: create a code in the control under Settings → Machines → Add Machine, then run:'
  printf '  sentinal connect %s\n' "$control_url"
}}
case "$link_state" in
  ACTIVE) printf '[4/4] Checking    sentinald running · connected to %s ✓\n' "${{link_control:-$control_url}}" ;;
  CONTROL_UNREACHABLE) printf '[4/4] Checking    sentinald running · linked to %s, which is not answering yet\n' "${{link_control:-$control_url}}" ;;
  REVOKED) printf '[4/4] Checking    sentinald running · REVOKED by the control\n'; connect_hint ;;
  NOT_CONNECTED) printf '[4/4] Checking    sentinald running · NOT CONNECTED\n'; connect_hint ;;
  *) printf '[4/4] Checking    installed; sentinald has not answered yet. Run: sentinal doctor\n' ;;
esac
printf 'Sentinal %s is installed.\n' "$release_version"
case ":${{PATH:-}}:" in
  *":$prefix/bin:"*) ;;
  *) printf 'Add %s to PATH to use the sentinal command, for example:\n  export PATH="%s:$PATH"\n' "$prefix/bin" "$prefix/bin" ;;
esac
"#,
        manifest.release_version,
        release_keys = release_keys,
    )
}
