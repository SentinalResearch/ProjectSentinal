#!/usr/bin/env bash
# Owner-only: sign one downloaded release's SHA256SUMS with a pinned release
# key. Runs on the owner's own machine after reviewing the draft GitHub
# Release; never in CI. The key is read only from --key (a file or a
# hardware-backed `sk` key handle); nothing is taken from the environment.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$SCRIPT_DIR/lib/release-signature.sh"

usage() {
  cat <<'EOF'
Usage: scripts/sign-release.sh --dir DIR --key KEY [--allowed-signers FILE]

  --dir DIR               directory holding SHA256SUMS and every file it lists
                          (e.g. `gh release download vX.Y.Z --dir DIR`)
  --key KEY               your release signing key (passphrase/touch prompts are normal)
  --allowed-signers FILE  pinned keys (default: packaging/release-keys/allowed_signers)
EOF
}

dir=""
key=""
allowed="$REPO_ROOT/packaging/release-keys/allowed_signers"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir) dir="${2:-}"; shift 2 ;;
    --key) key="${2:-}"; shift 2 ;;
    --allowed-signers) allowed="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) sentinal_signature_die "unknown argument '$1'" ;;
  esac
done
[[ -d "$dir" ]] || sentinal_signature_die "--dir must name a downloaded release directory"
[[ -f "$key" ]] || sentinal_signature_die "--key must name your release signing key file"
sentinal_release_keys_present "$allowed" ||
  sentinal_signature_die "no release signing keys are pinned in '$allowed'; pin your public key first"
sentinal_require_ssh_keygen_signatures

sums="$dir/SHA256SUMS"
sentinal_validate_sums_file "$sums"
while read -r expected name; do
  file="$dir/$name"
  [[ -f "$file" && ! -L "$file" ]] || sentinal_signature_die "$name is listed in SHA256SUMS but missing from '$dir'"
  if command -v sha256sum >/dev/null 2>&1; then actual="$(sha256sum "$file" | awk '{print $1}')"; else actual="$(shasum -a 256 "$file" | awk '{print $1}')"; fi
  [[ "$actual" == "$expected" ]] || sentinal_signature_die "$name does not match SHA256SUMS; refusing to sign"
done <"$sums"

signature="$sums.sig"
rm -f -- "$signature"
ssh-keygen -q -Y sign -n "$SENTINAL_RELEASE_NAMESPACE" -f "$key" "$sums" ||
  sentinal_signature_die "ssh-keygen could not sign SHA256SUMS"
if ! ssh-keygen -Y verify -f "$allowed" -I "$SENTINAL_RELEASE_IDENTITY" -n "$SENTINAL_RELEASE_NAMESPACE" \
  -s "$signature" <"$sums" >/dev/null 2>&1; then
  rm -f -- "$signature"
  sentinal_signature_die "this key is not pinned in '$allowed'; the signature was discarded"
fi

version="$(sed -n 's/.*sentinal-worker-\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)-.*/\1/p' "$sums" | head -n 1)"
echo "Signed $(wc -l <"$sums" | tr -d '[:space:]') files in $sums"
echo "Next:"
echo "  gh release upload v${version:-VERSION} '$signature' --clobber"
echo "  then publish the draft release on GitHub"
