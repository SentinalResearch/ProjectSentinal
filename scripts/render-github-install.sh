#!/usr/bin/env bash
# Render the GitHub bootstrap install.sh from packaging/github-install.sh.in,
# embedding only the pinned release-key lines and the repository name.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$SCRIPT_DIR/lib/release-signature.sh"

keys="$REPO_ROOT/packaging/release-keys/allowed_signers"
repo=""
output=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --keys) keys="${2:-}"; shift 2 ;;
    --repo) repo="${2:-}"; shift 2 ;;
    --output) output="${2:-}"; shift 2 ;;
    *) sentinal_signature_die "unknown argument '$1'" ;;
  esac
done
[[ "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || sentinal_signature_die "invalid --repo '$repo'; expected OWNER/NAME"
[[ -n "$output" ]] || sentinal_signature_die "--output is required"
sentinal_release_keys_present "$keys" ||
  sentinal_signature_die "no release signing keys are pinned in '$keys'; refusing to render an installer that trusts nothing"

key_lines="$(grep -E '^sentinal-release namespaces="sentinal-release" (ssh-ed25519|sk-ssh-ed25519@openssh.com) [A-Za-z0-9+/=]+' "$keys")"
temporary="$output.tmp.$$"
trap 'rm -f -- "$temporary"' EXIT
awk -v keys="$key_lines" -v repo="$repo" '
  $0 == "@SENTINAL_RELEASE_KEYS@" { print keys; next }
  { gsub(/@SENTINAL_REPO@/, repo); print }
' "$REPO_ROOT/packaging/github-install.sh.in" >"$temporary"
! grep -q '@SENTINAL_' "$temporary" || sentinal_signature_die "template placeholder left in rendered installer"
chmod 755 "$temporary"
mv -f "$temporary" "$output"
