#!/usr/bin/env bash
# Install this Machine's artifact of one already-published release from the
# control's private package directory: re-verify the stored owner signature,
# the artifact digest, and the archive entries, then run its installer. Used
# for the control's self-update after publishing and for explicit rollback.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$SCRIPT_DIR/lib/package-common.sh"
source "$SCRIPT_DIR/lib/release-signature.sh"

usage() {
  cat <<'EOF'
Usage: scripts/apply-release.sh --version VERSION [OPTIONS]

Options:
  --package-dir DIR       package directory (default: ~/.local/share/terminal-agent/packages)
  --allowed-signers FILE  pinned release keys (default: packaging/release-keys/allowed_signers)
  --rollback              permit installing an older release (explicit rollback)
  --no-service            pass --no-service to the installer
EOF
}

version=""
package_dir="${HOME:-}/.local/share/terminal-agent/packages"
allowed_signers="$REPO_ROOT/packaging/release-keys/allowed_signers"
installer_args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --package-dir) package_dir="${2:-}"; shift 2 ;;
    --allowed-signers) allowed_signers="${2:-}"; shift 2 ;;
    --rollback) rollback=true; shift ;;
    --no-service) installer_args+=(--no-service); shift ;;
    -h|--help) usage; exit 0 ;;
    *) sentinal_package_die "unknown apply argument '$1'" ;;
  esac
done
sentinal_package_valid_version "$version" || sentinal_package_die "invalid release version '$version'; expected MAJOR.MINOR.PATCH"
sentinal_package_require_absolute package "$package_dir"
[[ "${rollback:-false}" == false ]] || installer_args+=(--rollback "$version")

case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64) target=x86_64-unknown-linux-musl ;;
  Linux:aarch64|Linux:arm64) target=aarch64-unknown-linux-musl ;;
  Darwin:x86_64) target=x86_64-apple-darwin ;;
  Darwin:arm64) target=aarch64-apple-darwin ;;
  *) sentinal_package_die "this platform is unsupported: $(uname -s):$(uname -m)" ;;
esac

[[ -f "$package_dir/releases/$version.json" ]] || sentinal_package_die "release $version is not published in '$package_dir'"
sentinal_verify_release_sums "$package_dir/releases/$version.SHA256SUMS" "$package_dir/releases/$version.SHA256SUMS.sig" "$allowed_signers"
name="sentinal-worker-$version-$target.tar.gz"
artifact="$package_dir/artifacts/$name"
[[ -f "$artifact" && ! -L "$artifact" ]] || sentinal_package_die "release $version has no artifact for $target"
digest="$(sentinal_package_sha256 "$artifact")"
[[ "$(sentinal_sums_entry "$package_dir/releases/$version.SHA256SUMS" "$name")" == "$digest" ]] ||
  sentinal_package_die "stored $name does not match the signed SHA256SUMS; nothing was installed"

umask 022
work="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-apply.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT HUP INT TERM
while IFS= read -r entry; do
  case "$entry" in
    sentinal-worker|sentinal-worker/*) ;;
    *) sentinal_package_die "archive contains unsafe entry '$entry'" ;;
  esac
  case "/$entry/" in
    */../*|*/./*) sentinal_package_die "archive contains unsafe entry '$entry'" ;;
  esac
done < <(tar -tzf "$artifact")
tar -xzf "$artifact" -C "$work"
[[ -x "$work/sentinal-worker/scripts/install-worker.sh" ]] || sentinal_package_die "$name has no installer"

tailscale_cli="${SENTINAL_TAILSCALE_CLI:-$(command -v tailscale 2>/dev/null || true)}"
if [[ -z "$tailscale_cli" && -x /Applications/Tailscale.app/Contents/MacOS/Tailscale ]]; then
  tailscale_cli=/Applications/Tailscale.app/Contents/MacOS/Tailscale
fi
[[ -n "$tailscale_cli" ]] || sentinal_package_die "the Tailscale CLI is required"
bind_ip="$("$tailscale_cli" ip -4 2>/dev/null | awk 'NR == 1 { print; exit }')"

echo "Installing $version ($target) on this control"
"$work/sentinal-worker/scripts/install-worker.sh" install --release-dir "$work/sentinal-worker" \
  --bind "$bind_ip" --artifact-sha "$digest" ${installer_args[@]+"${installer_args[@]}"} >"$work/install.log" 2>&1 || {
  cat "$work/install.log" >&2
  sentinal_package_die "installing $version failed; the previous programs were restored"
}
echo "This control now runs $version"
