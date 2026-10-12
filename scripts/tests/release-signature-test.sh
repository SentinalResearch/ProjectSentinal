#!/usr/bin/env bash
# Release signing: the shared verification library and the owner's signing
# tool, with throwaway keys (real ssh-keygen, no network).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
signer="$repo_root/scripts/sign-release.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-release-signature-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "release signature test failed: $*" >&2
  exit 1
}

# Each check runs the library in a subshell so its exit-on-failure is observable.
lib() {
  bash -c 'source "$1/scripts/lib/release-signature.sh"; shift; "$@"' sentinal-lib "$repo_root" "$@"
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi
}

ssh-keygen -q -t ed25519 -N "" -C "pinned" -f "$test_root/pinned"
ssh-keygen -q -t ed25519 -N "" -C "stranger" -f "$test_root/stranger"
allowed="$test_root/allowed_signers"
printf 'sentinal-release namespaces="sentinal-release" %s\n' "$(cut -d' ' -f1,2 "$test_root/pinned.pub")" >"$allowed"
empty_allowed="$test_root/empty_allowed_signers"
printf '# no keys yet\n' >"$empty_allowed"

release="$test_root/release dir"
mkdir -p "$release"
printf 'linux bytes' >"$release/sentinal-worker-0.0.9-x86_64-unknown-linux-musl.tar.gz"
printf 'mac bytes' >"$release/sentinal-worker-0.0.9-aarch64-apple-darwin.tar.gz"
printf '#!/bin/sh\necho install\n' >"$release/install.sh"
(cd "$release" && for file in install.sh sentinal-worker-*; do printf '%s  %s\n' "$(sha256 "$file")" "$file"; done) >"$release/SHA256SUMS"

# The repository ships with no pinned key: every check must fail closed.
if lib sentinal_release_keys_present "$repo_root/packaging/release-keys/allowed_signers"; then
  grep -q "^sentinal-release " "$repo_root/packaging/release-keys/allowed_signers" ||
    fail 'keys reported present in a file without key lines'
fi
lib sentinal_release_keys_present "$allowed" || fail 'a pinned key was not recognised'
! lib sentinal_release_keys_present "$empty_allowed" || fail 'an empty allowed_signers counted as pinned'

# 1. The owner signs with the pinned key; the signature verifies.
output="$("$signer" --dir "$release" --key "$test_root/pinned" --allowed-signers "$allowed")" ||
  fail "signing with the pinned key failed: $output"
[[ -s "$release/SHA256SUMS.sig" ]] || fail 'no signature was written'
[[ "$output" == *"gh release upload"* ]] || fail "no upload instruction: $output"
lib sentinal_verify_release_sums "$release/SHA256SUMS" "$release/SHA256SUMS.sig" "$allowed" ||
  fail 'a valid signature did not verify'
[[ "$(lib sentinal_sums_entry "$release/SHA256SUMS" install.sh)" == "$(sha256 "$release/install.sh")" ]] ||
  fail 'sums entry lookup is wrong'
[[ -z "$(lib sentinal_sums_entry "$release/SHA256SUMS" missing.tar.gz)" ]] || fail 'a missing entry returned a digest'

# 2. A key that is not pinned is refused and leaves no signature behind.
rm -f "$release/SHA256SUMS.sig"
if error="$("$signer" --dir "$release" --key "$test_root/stranger" --allowed-signers "$allowed" 2>&1)"; then
  fail 'signing with an unpinned key succeeded'
fi
[[ "$error" == *"not pinned"* ]] || fail "unclear unpinned-key error: $error"
[[ ! -e "$release/SHA256SUMS.sig" ]] || fail 'an unpinned signature was left behind'

# 3. A file that does not match SHA256SUMS is refused before signing.
printf 'tampered' >>"$release/install.sh"
if error="$("$signer" --dir "$release" --key "$test_root/pinned" --allowed-signers "$allowed" 2>&1)"; then
  fail 'signed a release whose file does not match SHA256SUMS'
fi
[[ "$error" == *"install.sh does not match"* ]] || fail "unclear mismatch error: $error"
(cd "$release" && for file in install.sh sentinal-worker-*; do printf '%s  %s\n' "$(sha256 "$file")" "$file"; done) >"$release/SHA256SUMS"
"$signer" --dir "$release" --key "$test_root/pinned" --allowed-signers "$allowed" >/dev/null

# 4. Verification refuses tampering, a missing signature, no pinned keys, and malformed lists.
cp "$release/SHA256SUMS" "$test_root/sums.good"
printf '%s  %s\n' "$(sha256 "$release/install.sh")" extra.tar.gz >>"$release/SHA256SUMS"
if error="$(lib sentinal_verify_release_sums "$release/SHA256SUMS" "$release/SHA256SUMS.sig" "$allowed" 2>&1)"; then
  fail 'a tampered SHA256SUMS verified'
fi
[[ "$error" == *"not signed by a trusted Sentinal release key"* ]] || fail "unclear tamper error: $error"
cp "$test_root/sums.good" "$release/SHA256SUMS"

if error="$(lib sentinal_verify_release_sums "$release/SHA256SUMS" "$test_root/absent.sig" "$allowed" 2>&1)"; then
  fail 'a missing signature verified'
fi
[[ "$error" == *"not signed by the owner"* ]] || fail "unclear missing-signature error: $error"

if error="$(lib sentinal_verify_release_sums "$release/SHA256SUMS" "$release/SHA256SUMS.sig" "$empty_allowed" 2>&1)"; then
  fail 'verified with no pinned keys'
fi
[[ "$error" == *"no release signing keys are pinned"* ]] || fail "unclear no-keys error: $error"

for bad in 'not a sums line' "$(sha256 "$release/install.sh")  ../escape" "$(sha256 "$release/install.sh") install.sh"; do
  printf '%s\n' "$bad" >"$test_root/bad-sums"
  if lib sentinal_validate_sums_file "$test_root/bad-sums" 2>/dev/null; then
    fail "accepted malformed SHA256SUMS line: $bad"
  fi
done
cat "$test_root/sums.good" "$test_root/sums.good" >"$test_root/dup-sums"
if error="$(lib sentinal_validate_sums_file "$test_root/dup-sums" 2>&1)"; then
  fail 'accepted a SHA256SUMS that names a file twice'
fi
[[ "$error" == *"more than once"* ]] || fail "unclear duplicate error: $error"

# The signer never runs in CI and never reads a key from the environment.
if grep -Eq 'GITHUB_|SENTINAL_RELEASE_KEY|secrets\.' "$signer"; then
  fail 'the signer must take its key only from --key on the owner machine'
fi

echo 'release signature test: passed'
