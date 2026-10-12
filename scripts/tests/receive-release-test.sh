#!/usr/bin/env bash
# Offline tests for scripts/receive-release.sh: fake curl and gh, real
# packager/publisher, temporary package directory and HOME.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
receiver="$repo_root/scripts/receive-release.sh"
builder="$repo_root/scripts/build-worker-release.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-receive-release-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "receive release test failed: $*" >&2
  exit 1
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi
}

targets="$(sed -n 's/^ *\([a-z0-9_]*-[a-z0-9_-]*\)) printf .*/\1/p' "$repo_root/scripts/lib/package-common.sh")"
[[ -n "$targets" ]] || fail 'could not read supported targets'

# Real artifacts for 0.0.9 built from fake binaries.
bins="$test_root/bins"
mkdir -p "$bins"
printf '#!/bin/sh\necho sentinal\n' >"$bins/sentinal"
printf '#!/bin/sh\necho sentinald\n' >"$bins/sentinald"
chmod 755 "$bins/sentinal" "$bins/sentinald"
good="$test_root/github good"
mkdir -p "$good"
for target in $targets; do
  "$builder" --version 0.0.9 --target "$target" --bin-dir "$bins" --output "$good" \
    --source-date-epoch 1790712009 >/dev/null
done
printf '#!/bin/sh\necho bootstrap\n' >"$good/install.sh"

# Throwaway release keys: one pinned, one not.
ssh-keygen -q -t ed25519 -N "" -C pinned -f "$test_root/release-key"
ssh-keygen -q -t ed25519 -N "" -C stranger -f "$test_root/stranger-key"
allowed="$test_root/allowed_signers"
printf 'sentinal-release namespaces="sentinal-release" %s\n' "$(cut -d' ' -f1,2 "$test_root/release-key.pub")" >"$allowed"

# Sign DIR's existing SHA256SUMS with KEY (default: the pinned key).
sign_sums() {
  rm -f "$1/SHA256SUMS.sig"
  ssh-keygen -q -Y sign -n sentinal-release -f "${2:-$test_root/release-key}" "$1/SHA256SUMS"
}
(cd "$good" && for file in install.sh *.tar.gz; do printf '%s  %s\n' "$(sha256 "$file")" "$file"; done) >"$good/SHA256SUMS"
sign_sums "$good"

fake_bin="$test_root/fake bin"
mkdir -p "$fake_bin"
cat >"$fake_bin/curl" <<'EOF'
#!/bin/sh
output=''
url=''
follows_redirects=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    --location) follows_redirects=true; shift ;;
    https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
printf '%s\n' "$url" >>"$SENTINAL_TEST_CURL_LOG"
# Like GitHub: release downloads redirect; without --location curl saves the
# empty redirect body and still exits 0.
if [ "$follows_redirects" = false ] || [ "${SENTINAL_TEST_EMPTY_BODY:-}" = 1 ]; then
  : >"$output"
  exit 0
fi
case "$url" in
  https://github.com/example/sentinal/releases/download/v0.0.9/*) ;;
  *) exit 22 ;;
esac
source="$SENTINAL_TEST_RELEASE_DIR/${url##*/}"
[ -f "$source" ] || exit 22
cp "$source" "$output"
EOF
cat >"$fake_bin/gh" <<'EOF'
#!/bin/sh
case " $* " in
  *" --help "*) exit 0 ;;
esac
[ "$1 $2" = 'attestation verify' ] || exit 2
printf '%s\n' "$*" >>"$SENTINAL_TEST_GH_LOG"
exit "${SENTINAL_TEST_GH_FAIL:-0}"
EOF
cat >"$fake_bin/tailscale" <<'EOF'
#!/bin/sh
[ "$1 $2" = 'ip -4' ] && printf '%s\n' 100.90.80.79
EOF
chmod 755 "$fake_bin/curl" "$fake_bin/gh" "$fake_bin/tailscale"
no_gh_bin="$test_root/no gh bin"
mkdir -p "$no_gh_bin"
cp "$fake_bin/curl" "$fake_bin/tailscale" "$no_gh_bin/"

export SENTINAL_TEST_CURL_LOG="$test_root/curl.log"
export SENTINAL_TEST_GH_LOG="$test_root/gh.log"
base_path="/usr/bin:/bin:/usr/sbin:/sbin"

receive() {
  local release_dir="$1" path_dir="$2"
  shift 2
  SENTINAL_TEST_RELEASE_DIR="$release_dir" PATH="$path_dir:$base_path" HOME="$test_root/home" \
    "$receiver" --repo example/sentinal --allowed-signers "$allowed" "$@"
}

# 1. Signed, complete release: verified, published with every target.
packages="$test_root/packages ok"
: >"$SENTINAL_TEST_GH_LOG"
output="$(receive "$good" "$fake_bin" --version 0.0.9 --package-dir "$packages" --no-self-update)" ||
  fail 'a valid release was refused'
[[ "$output" == *"Published Sentinal worker 0.0.9 to stable"* ]] || fail "missing publish confirmation: $output"
stable="$packages/channels/stable.json"
grep -q '"release_version": "0.0.9"' "$stable" || fail 'stable does not name 0.0.9'
for target in $targets; do
  grep -q "\"target\": \"$target\"" "$stable" || fail "stable lacks $target"
  grep -q "sentinal-worker-0.0.9-$target.tar.gz --repo example/sentinal" "$SENTINAL_TEST_GH_LOG" ||
    fail "provenance was not verified for $target"
done
if grep -v '^https://github.com/example/sentinal/releases/download/v0.0.9/' "$SENTINAL_TEST_CURL_LOG" | grep -q .; then
  fail 'downloaded from somewhere other than the exact release'
fi

# 1b. An empty checksum list (e.g. an unfollowed redirect) is refused clearly.
if error="$(SENTINAL_TEST_EMPTY_BODY=1 receive "$good" "$fake_bin" --version 0.0.9 --package-dir "$test_root/packages empty" --no-self-update 2>&1)"; then
  fail 'an empty SHA256SUMS was accepted'
fi
[[ "$error" == *"SHA256SUMS is empty or malformed"* ]] || fail "unclear empty-download error: $error"

# 2. A tampered artifact is refused and nothing is published.
tampered="$test_root/github tampered"
cp -R "$good" "$tampered"
printf 'x' >>"$tampered/sentinal-worker-0.0.9-aarch64-apple-darwin.tar.gz"
packages="$test_root/packages tampered"
if error="$(receive "$tampered" "$fake_bin" --version 0.0.9 --package-dir "$packages" --no-self-update 2>&1)"; then
  fail 'a tampered artifact was accepted'
fi
[[ "$error" == *"SHA-256 mismatch"* ]] || fail "unclear tamper error: $error"
[[ ! -e "$packages/channels/stable.json" ]] || fail 'a refused release moved stable'

# 3. A failed provenance check is refused.
packages="$test_root/packages unsigned"
if error="$(SENTINAL_TEST_GH_FAIL=1 receive "$good" "$fake_bin" --version 0.0.9 --package-dir "$packages" --no-self-update 2>&1)"; then
  fail 'a release that failed attestation was accepted'
fi
[[ "$error" == *"provenance"* ]] || fail "unclear attestation error: $error"
[[ ! -e "$packages/channels/stable.json" ]] || fail 'an unverified release moved stable'

# 4. Without gh the receiver refuses unless --skip-provenance is explicit; the
#    owner signature is still required. --hash-only no longer exists.
packages="$test_root/packages no gh"
if error="$(receive "$good" "$no_gh_bin" --version 0.0.9 --package-dir "$packages" --no-self-update 2>&1)"; then
  fail 'published without provenance and without --skip-provenance'
fi
[[ "$error" == *"--skip-provenance"* ]] || fail "no guidance without gh: $error"
if error="$(receive "$good" "$no_gh_bin" --version 0.0.9 --package-dir "$packages" --no-self-update --hash-only 2>&1)"; then
  fail '--hash-only still bypasses verification'
fi
[[ "$error" == *"signatures are always required"* ]] || fail "unclear --hash-only error: $error"
output="$(receive "$good" "$no_gh_bin" --version 0.0.9 --package-dir "$packages" --no-self-update --skip-provenance 2>&1)" ||
  fail "--skip-provenance was refused: $output"
[[ "$output" == *"without GitHub build provenance"* ]] || fail "--skip-provenance did not warn: $output"

# 4b. A missing signature or one from an unpinned key is refused.
unsigned="$test_root/github unsigned"
cp -R "$good" "$unsigned"
rm -f "$unsigned/SHA256SUMS.sig"
if error="$(receive "$unsigned" "$fake_bin" --version 0.0.9 --package-dir "$test_root/packages nosig" --no-self-update 2>&1)"; then
  fail 'an unsigned release was received'
fi
[[ "$error" == *"SHA256SUMS.sig"* ]] || fail "unclear missing-signature error: $error"
sign_sums "$unsigned" "$test_root/stranger-key"
if error="$(receive "$unsigned" "$fake_bin" --version 0.0.9 --package-dir "$test_root/packages stranger" --no-self-update 2>&1)"; then
  fail 'a release signed by an unpinned key was received'
fi
[[ "$error" == *"not signed by a trusted Sentinal release key"* ]] || fail "unclear unpinned-key error: $error"
[[ ! -e "$test_root/packages stranger/channels/stable.json" ]] || fail 'an untrusted release moved stable' 

# 5. An archive whose BUILD.json names another version is refused.
mismatch="$test_root/github mismatch"
mkdir -p "$mismatch"
"$builder" --version 0.0.8 --target x86_64-unknown-linux-musl --bin-dir "$bins" --output "$mismatch" \
  --source-date-epoch 1790712008 >/dev/null
cp "$good"/*.tar.gz "$mismatch/"
mv "$mismatch/sentinal-worker-0.0.8-x86_64-unknown-linux-musl.tar.gz" \
  "$mismatch/sentinal-worker-0.0.9-x86_64-unknown-linux-musl.tar.gz"
(cd "$mismatch" && for file in *.tar.gz; do printf '%s  %s\n' "$(sha256 "$file")" "$file"; done) >"$mismatch/SHA256SUMS"
sign_sums "$mismatch"
if error="$(receive "$mismatch" "$fake_bin" --version 0.0.9 --package-dir "$test_root/packages mismatch" --no-self-update 2>&1)"; then
  fail 'an archive for another version was accepted'
fi
[[ "$error" == *"BUILD.json"* ]] || fail "unclear version mismatch error: $error"

# 6. A checksum list that omits a supported target is refused.
partial="$test_root/github partial"
cp -R "$good" "$partial"
grep -v 'aarch64-unknown-linux-musl' "$good/SHA256SUMS" >"$partial/SHA256SUMS"
sign_sums "$partial"
if error="$(receive "$partial" "$fake_bin" --version 0.0.9 --package-dir "$test_root/packages partial" --no-self-update 2>&1)"; then
  fail 'an incomplete release was accepted'
fi
[[ "$error" == *"aarch64-unknown-linux-musl"* ]] || fail "missing target not named: $error"

# 7. The control updates itself from the verified artifact for this host.
mkdir -p "$test_root/home"
output="$(receive "$good" "$fake_bin" --version 0.0.9 --package-dir "$test_root/packages self" --no-service 2>&1)" ||
  fail "self-update failed: $output"
[[ -x "$test_root/home/.local/bin/sentinald" ]] || fail 'self-update did not install sentinald'
grep -q '"version": "0.0.9"' "$test_root/home/.local/share/sentinal/release.json" ||
  fail 'self-update did not record the release'
[[ "$output" == *"This control now runs 0.0.9"* ]] || fail "no self-update confirmation: $output"

# 8. apply-release.sh reinstalls from the local package directory, re-verifying
#    the stored signature and digest; tampering installs nothing.
applier="$repo_root/scripts/apply-release.sh"
self_packages="$test_root/packages self"
apply() {
  PATH="$fake_bin:$base_path" HOME="$test_root/home" "$applier" --package-dir "$self_packages" \
    --allowed-signers "$allowed" --no-service "$@"
}
output="$(apply --version 0.0.9 2>&1)" || fail "apply-release of a published release failed: $output"
[[ "$output" == *"This control now runs 0.0.9"* ]] || fail "no apply confirmation: $output"
host_artifact="$(ls "$self_packages/artifacts/" | grep "$(uname -s | tr 'A-Z' 'a-z' | sed 's/darwin/apple-darwin/;s/linux/linux-musl/')" | head -n 1)"
cp "$self_packages/artifacts/$host_artifact" "$test_root/artifact.bak"
chmod 600 "$self_packages/artifacts/$host_artifact"
printf 'x' >>"$self_packages/artifacts/$host_artifact"
if error="$(apply --version 0.0.9 2>&1)"; then
  fail 'apply-release installed a tampered stored artifact'
fi
[[ "$error" == *"does not match the signed SHA256SUMS"* ]] || fail "unclear apply tamper error: $error"
cp "$test_root/artifact.bak" "$self_packages/artifacts/$host_artifact"
mv "$self_packages/releases/0.0.9.SHA256SUMS.sig" "$test_root/sig.bak"
if error="$(apply --version 0.0.9 2>&1)"; then
  fail 'apply-release installed a release without its stored signature'
fi
[[ "$error" == *"not signed by the owner"* ]] || fail "unclear apply missing-signature error: $error"
mv "$test_root/sig.bak" "$self_packages/releases/0.0.9.SHA256SUMS.sig"
if apply --version 0.0.8 >/dev/null 2>&1; then
  fail 'apply-release installed an unpublished release'
fi

# Bad input is refused before any download.
: >"$SENTINAL_TEST_CURL_LOG"
for bad_version in 'v0.0.9;rm' '0.0.9.' '0.0.9..' '.0.0.9' '0.0' '0.0.9.1' '00.0.9' '0.0.9 '; do
  if receive "$good" "$fake_bin" --version "$bad_version" --package-dir "$test_root/p" --no-self-update 2>/dev/null; then
    fail "invalid version '$bad_version' was accepted"
  fi
done
[[ ! -s "$SENTINAL_TEST_CURL_LOG" ]] || fail 'downloaded before validating the version'

echo 'receive release test: passed'
