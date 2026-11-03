#!/usr/bin/env bash
# macOS: installed programs are signed with one local identity under a fixed
# identifier, so a rebuilt release keeps the same designated requirement and
# privacy grants survive the update. Runs against a throwaway keychain.
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo "macOS signing test skipped (not macOS)"
  exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repo_root/scripts/lib/install-common.sh"
source "$repo_root/scripts/lib/macos-signing.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-signing-test.XXXXXX")"
export SENTINAL_SIGNING_KEYCHAIN="$test_root/test.keychain-db"
cleanup() {
  security delete-keychain "$SENTINAL_SIGNING_KEYCHAIN" >/dev/null 2>&1 || true
  rm -rf "$test_root"
}
trap cleanup EXIT

fail() {
  echo "macOS signing test failed: $*" >&2
  exit 1
}

security create-keychain -p sentinal-test "$SENTINAL_SIGNING_KEYCHAIN"
security unlock-keychain -p sentinal-test "$SENTINAL_SIGNING_KEYCHAIN"

requirement() { codesign -dr - "$1" 2>&1 | sed -n 's/^designated => //p'; }

# Two "releases": different bytes, so different ad-hoc hashes.
mkdir -p "$test_root/v1" "$test_root/v2" "$test_root/bin"
cc_source="$test_root/main.c"
printf 'int main(void){return 0;}\n' >"$cc_source"
clang -o "$test_root/v1/sentinald" "$cc_source"
printf 'int main(void){return 1;}\n' >"$cc_source"
clang -o "$test_root/v2/sentinald" "$cc_source"

sentinal_sign_program=sentinal_signing_sign
sentinal_atomic_install "$test_root/v1/sentinald" "$test_root/bin/sentinald" >/dev/null
first="$(requirement "$test_root/bin/sentinald")"
[[ "$first" == 'identifier "dev.sentinal.sentinald" and certificate leaf = H"'* ]] ||
  fail "unexpected designated requirement: $first"
codesign --verify "$test_root/bin/sentinald" || fail 'the signature does not verify'

sentinal_atomic_install "$test_root/v2/sentinald" "$test_root/bin/sentinald" >/dev/null
second="$(requirement "$test_root/bin/sentinald")"
[[ "$second" == "$first" ]] || fail "the requirement changed between releases: $first -> $second"
"$test_root/bin/sentinald" && fail 'the second release was not installed'

count="$(security find-identity -p codesigning "$SENTINAL_SIGNING_KEYCHAIN" | grep -c "\"$SENTINAL_SIGNING_NAME\"" || true)"
[[ "$count" == 1 ]] || fail "expected one identity, found $count"

echo "macOS signing test passed"
