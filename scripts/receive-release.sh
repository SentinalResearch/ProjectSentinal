#!/usr/bin/env bash
# Receive one GitHub release on the control: download every supported target,
# verify SHA256SUMS, BUILD.json, and GitHub build provenance, publish to the
# private Tailnet package channel, then update this control itself.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/package-common.sh"

readonly TARGETS=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin)
readonly MAX_SUMS_BYTES=65536
readonly MAX_ARTIFACT_BYTES=268435456

usage() {
  cat <<'EOF'
Usage: scripts/receive-release.sh --version VERSION [OPTIONS]

Options:
  --repo OWNER/NAME     GitHub repository (default: SentinalResearch/ProjectSentinal)
  --package-dir DIR     package directory (default: ~/.local/share/terminal-agent/packages)
  --hash-only           publish without GitHub build-provenance verification (not recommended)
  --no-self-update      publish only; do not update this control's own programs
  --no-service          pass --no-service to the self-update installer
EOF
}

version=""
repo="SentinalResearch/ProjectSentinal"
package_dir="${HOME:-}/.local/share/terminal-agent/packages"
hash_only=false
self_update=true
installer_args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --repo) repo="${2:-}"; shift 2 ;;
    --package-dir) package_dir="${2:-}"; shift 2 ;;
    --hash-only) hash_only=true; shift ;;
    --no-self-update) self_update=false; shift ;;
    --no-service) installer_args+=(--no-service); shift ;;
    -h|--help) usage; exit 0 ;;
    *) sentinal_package_die "unknown receive argument '$1'" ;;
  esac
done

version="${version#v}"
sentinal_package_valid_version "$version" || sentinal_package_die "invalid release version '$version'; expected MAJOR.MINOR.PATCH"
[[ "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || sentinal_package_die "invalid repository '$repo'"
sentinal_package_require_absolute package "$package_dir"
command -v curl >/dev/null 2>&1 || sentinal_package_die "curl is required"

if [[ "$hash_only" == false ]]; then
  if ! command -v gh >/dev/null 2>&1 || ! gh attestation verify --help >/dev/null 2>&1; then
    sentinal_package_die "GitHub CLI 2.49+ is required to verify build provenance (gh attestation verify).
Install it from https://cli.github.com, or re-run with --hash-only to publish on checksums alone."
  fi
fi

umask 022
work="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-receive.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT HUP INT TERM
base="https://github.com/$repo/releases/download/v$version"

download() {
  local name="$1" limit="$2"
  # GitHub serves release assets through a redirect: follow it (HTTPS only).
  curl --location --proto '=https' --proto-redir '=https' --tlsv1.2 --max-redirs 3 --fail --silent --show-error \
    --max-filesize "$limit" --output "$work/$name" "$base/$name" ||
    sentinal_package_die "could not download $name from release v$version"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi
}

echo "Receiving Sentinal $version from $repo"
download SHA256SUMS "$MAX_SUMS_BYTES"
# curl reports success for an empty body, so require well-formed lines.
if [[ ! -s "$work/SHA256SUMS" ]] || grep -qvE '^[0-9a-f]{64}  [A-Za-z0-9._-]+$' "$work/SHA256SUMS"; then
  sentinal_package_die "SHA256SUMS is empty or malformed; nothing was published"
fi
expected_names=()
for target in "${TARGETS[@]}"; do
  expected_names+=("sentinal-worker-$version-$target.tar.gz")
done
while read -r _ name; do
  [[ -z "$name" ]] && continue
  printf '%s\n' "${expected_names[@]}" | grep -qxF -- "$name" ||
    sentinal_package_die "SHA256SUMS lists unexpected file '$name'"
done <"$work/SHA256SUMS"

artifacts=()
for target in "${TARGETS[@]}"; do
  name="sentinal-worker-$version-$target.tar.gz"
  expected="$(awk -v name="$name" '$2 == name { print $1 }' "$work/SHA256SUMS")"
  [[ "$expected" =~ ^[0-9a-f]{64}$ ]] || sentinal_package_die "SHA256SUMS has no entry for $target"
  download "$name" "$MAX_ARTIFACT_BYTES"
  [[ "$(sha256_of "$work/$name")" == "$expected" ]] || sentinal_package_die "SHA-256 mismatch for $name; nothing was published"
  build_json="$(tar -xzOf "$work/$name" sentinal-worker/BUILD.json 2>/dev/null)" ||
    sentinal_package_die "$name has no sentinal-worker/BUILD.json"
  built_version="$(printf '%s\n' "$build_json" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  built_target="$(printf '%s\n' "$build_json" | sed -n 's/.*"target"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  [[ "$built_version" == "$version" && "$built_target" == "$target" ]] ||
    sentinal_package_die "$name BUILD.json names $built_version/$built_target, expected $version/$target"
  if [[ "$hash_only" == false ]]; then
    gh attestation verify "$work/$name" --repo "$repo" >/dev/null 2>&1 ||
      sentinal_package_die "build provenance verification failed for $name; nothing was published"
  fi
  artifacts+=(--artifact "$work/$name")
  echo "  verified $target"
done
if [[ "$hash_only" == true ]]; then
  echo "WARNING: published WITHOUT build provenance verification (--hash-only); checksums came from the same release."
fi

"$SCRIPT_DIR/publish-worker-release.sh" publish --version "$version" --package-dir "$package_dir" "${artifacts[@]}"

if [[ "$self_update" == false ]]; then
  exit 0
fi
case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64) host_target=x86_64-unknown-linux-musl ;;
  Linux:aarch64|Linux:arm64) host_target=aarch64-unknown-linux-musl ;;
  Darwin:x86_64) host_target=x86_64-apple-darwin ;;
  Darwin:arm64) host_target=aarch64-apple-darwin ;;
  *) sentinal_package_die "published, but this control's platform is unsupported for self-update" ;;
esac
tailscale_cli="$(command -v tailscale 2>/dev/null || true)"
if [[ -z "$tailscale_cli" && -x /Applications/Tailscale.app/Contents/MacOS/Tailscale ]]; then
  tailscale_cli=/Applications/Tailscale.app/Contents/MacOS/Tailscale
fi
[[ -n "$tailscale_cli" ]] || sentinal_package_die "published, but the Tailscale CLI is missing for self-update"
bind_ip="$("$tailscale_cli" ip -4 2>/dev/null | awk 'NR == 1 { print; exit }')"
mkdir "$work/self"
tar -xzf "$work/sentinal-worker-$version-$host_target.tar.gz" -C "$work/self"
echo "Updating this control to $version ($host_target)"
"$work/self/sentinal-worker/scripts/install-worker.sh" install --release-dir "$work/self/sentinal-worker" \
  --bind "$bind_ip" --artifact-sha "$(sha256_of "$work/sentinal-worker-$version-$host_target.tar.gz")" \
  ${installer_args[@]+"${installer_args[@]}"} >"$work/install.log" 2>&1 || {
  cat "$work/install.log" >&2
  sentinal_package_die "published, but updating this control failed; its previous programs were restored"
}
echo "This control now runs $version"
