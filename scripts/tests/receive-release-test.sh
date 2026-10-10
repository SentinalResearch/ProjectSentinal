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
(cd "$good" && for file in *.tar.gz; do printf '%s  %s\n' "$(sha256 "$file")" "$file"; done) >"$good/SHA256SUMS"

fake_bin="$test_root/fake bin"
mkdir -p "$fake_bin"
cat >"$fake_bin/curl" <<'EOF'
#!/bin/sh
output=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
printf '%s\n' "$url" >>"$SENTINAL_TEST_CURL_LOG"
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
    "$receiver" --repo example/sentinal "$@"
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

# 4. Without gh the receiver refuses unless --hash-only is explicit.
packages="$test_root/packages no gh"
if error="$(receive "$good" "$no_gh_bin" --version 0.0.9 --package-dir "$packages" --no-self-update 2>&1)"; then
  fail 'published without provenance and without --hash-only'
fi
[[ "$error" == *"--hash-only"* ]] || fail "no guidance without gh: $error"
output="$(receive "$good" "$no_gh_bin" --version 0.0.9 --package-dir "$packages" --no-self-update --hash-only 2>&1)" ||
  fail '--hash-only was refused'
[[ "$output" == *"WITHOUT build provenance"* ]] || fail "--hash-only did not warn: $output"

# 5. An archive whose BUILD.json names another version is refused.
mismatch="$test_root/github mismatch"
mkdir -p "$mismatch"
"$builder" --version 0.0.8 --target x86_64-unknown-linux-musl --bin-dir "$bins" --output "$mismatch" \
  --source-date-epoch 1790712008 >/dev/null
cp "$good"/*.tar.gz "$mismatch/"
mv "$mismatch/sentinal-worker-0.0.8-x86_64-unknown-linux-musl.tar.gz" \
  "$mismatch/sentinal-worker-0.0.9-x86_64-unknown-linux-musl.tar.gz"
(cd "$mismatch" && for file in *.tar.gz; do printf '%s  %s\n' "$(sha256 "$file")" "$file"; done) >"$mismatch/SHA256SUMS"
if error="$(receive "$mismatch" "$fake_bin" --version 0.0.9 --package-dir "$test_root/packages mismatch" --no-self-update 2>&1)"; then
  fail 'an archive for another version was accepted'
fi
[[ "$error" == *"BUILD.json"* ]] || fail "unclear version mismatch error: $error"

# 6. A checksum list that omits a supported target is refused.
partial="$test_root/github partial"
cp -R "$good" "$partial"
grep -v 'aarch64-unknown-linux-musl' "$good/SHA256SUMS" >"$partial/SHA256SUMS"
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

# Bad input is refused before any download.
: >"$SENTINAL_TEST_CURL_LOG"
if receive "$good" "$fake_bin" --version 'v0.0.9;rm' --package-dir "$test_root/p" --no-self-update 2>/dev/null; then
  fail 'an invalid version was accepted'
fi
[[ ! -s "$SENTINAL_TEST_CURL_LOG" ]] || fail 'downloaded before validating the version'

echo 'receive release test: passed'
