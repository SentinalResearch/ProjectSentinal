//! Generated, bounded worker bootstrap script.

use super::manifest::{ReleaseManifest, TargetArch, TargetOs};

/// Generate a bootstrap script from an already validated manifest.
pub(super) fn generate(base_url: &str, manifest: &ReleaseManifest) -> String {
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
command -v tailscale >/dev/null 2>&1 || fail "Tailscale is required and must be connected"

temporary="$(mktemp -d "${{TMPDIR:-/tmp}}/sentinal-bootstrap.XXXXXX")" || fail "cannot create temporary directory"
trap 'rm -rf -- "$temporary"' EXIT HUP INT TERM
archive="$temporary/worker.tar.gz"

curl --proto '=https' --proto-redir '=https' --max-redirs 0 --fail --silent --show-error \
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

bind_ip="$(tailscale ip -4 2>/dev/null | awk 'NR == 1 {{ print; exit }}')"
case "$bind_ip" in
  100.*) ;;
  *) fail "Tailscale did not return a 100.64.0.0/10 address" ;;
esac
second="$(printf '%s' "$bind_ip" | awk -F. '{{print $2}}')"
[ "$second" -ge 64 ] 2>/dev/null && [ "$second" -le 127 ] 2>/dev/null || fail "Tailscale address '$bind_ip' is outside 100.64.0.0/10"

set --
[ -z "$rollback" ] || set -- "$@" --rollback "$rollback"
[ "$prefix_explicit" = 'false' ] || set -- "$@" --prefix "$prefix"
"$release/scripts/install-worker.sh" install --release-dir "$release" --bind "$bind_ip" \
  --control-url "$control_url" --artifact-sha "$expected_sha" "$@"
printf '%s\n' 'Sentinal worker installed and running'
printf '%s\n' 'State: UNPAIRED'
printf '%s\n' 'Next (available in Milestone 4.0):'
printf '  sentinal connect %s\n' "$control_url"
"#,
        manifest.release_version
    )
}
