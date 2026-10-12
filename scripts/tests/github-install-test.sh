#!/usr/bin/env bash
# The GitHub bootstrap install.sh (first control or offline worker): rendered
# with pinned keys, it installs only an owner-signed release for this host.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
renderer="$repo_root/scripts/render-github-install.sh"
builder="$repo_root/scripts/build-worker-release.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-github-install-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "github install test failed: $*" >&2
  exit 1
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi
}

case "$(uname -s):$(uname -m)" in
  Linux:x86_64) host=x86_64-unknown-linux-musl ;;
  Linux:aarch64|Linux:arm64) host=aarch64-unknown-linux-musl ;;
  Darwin:x86_64) host=x86_64-apple-darwin ;;
  Darwin:arm64) host=aarch64-apple-darwin ;;
  *) echo 'github install test: skipped (unsupported test host)'; exit 0 ;;
esac

ssh-keygen -q -t ed25519 -N "" -C pinned -f "$test_root/release-key"
ssh-keygen -q -t ed25519 -N "" -C stranger -f "$test_root/stranger-key"
keys="$test_root/allowed_signers"
printf '# pinned\nsentinal-release namespaces="sentinal-release" %s\n' "$(cut -d' ' -f1,2 "$test_root/release-key.pub")" >"$keys"
printf '# none yet\n' >"$test_root/no_keys"

# A signed v0.0.9 release for this host, laid out like a GitHub Release.
bins="$test_root/bins"
mkdir -p "$bins"
printf '#!/bin/sh\necho sentinal\n' >"$bins/sentinal"
printf '#!/bin/sh\necho sentinald\n' >"$bins/sentinald"
chmod 755 "$bins/sentinal" "$bins/sentinald"
release="$test_root/github release"
mkdir -p "$release"
"$builder" --version 0.0.9 --target "$host" --bin-dir "$bins" --output "$release" --source-date-epoch 1790712009 >/dev/null
"$renderer" --keys "$keys" --repo example/sentinal --output "$release/install.sh"
(cd "$release" && for file in install.sh *.tar.gz; do printf '%s  %s\n' "$(sha256 "$file")" "$file"; done) >"$release/SHA256SUMS"
ssh-keygen -q -Y sign -n sentinal-release -f "$test_root/release-key" "$release/SHA256SUMS"

# The rendered script embeds exactly the key lines, the repository, and no secrets.
grep -Fq "$(cut -d' ' -f2 "$test_root/release-key.pub")" "$release/install.sh" || fail 'pinned key not embedded'
grep -Fq "repo='example/sentinal'" "$release/install.sh" || fail 'repository not embedded'
! grep -q '@SENTINAL_' "$release/install.sh" || fail 'template placeholder left in rendered script'
if "$renderer" --keys "$test_root/no_keys" --repo example/sentinal --output "$test_root/unpinned.sh" 2>/dev/null; then
  fail 'rendered an installer with no pinned keys'
fi

fake_bin="$test_root/fake bin"
mkdir -p "$fake_bin"
cat >"$fake_bin/curl" <<'EOF'
#!/bin/sh
output=''
url=''
follows=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    --location) follows=true; shift ;;
    https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
printf '%s\n' "$url" >>"$SENTINAL_TEST_CURL_LOG"
case "$url" in
  https://github.com/example/sentinal/releases/latest/download/*|https://github.com/example/sentinal/releases/download/v0.0.9/*) ;;
  *) exit 22 ;;
esac
# GitHub redirects release assets; without --location the body is empty.
if [ "$follows" = false ]; then : >"$output"; exit 0; fi
source="$SENTINAL_TEST_RELEASE_DIR/${url##*/}"
[ -f "$source" ] || exit 22
cp "$source" "$output"
EOF
cat >"$fake_bin/tailscale" <<'EOF'
#!/bin/sh
[ "$1 $2" = 'ip -4' ] && printf '%s\n' 100.90.80.77
EOF
chmod 755 "$fake_bin/curl" "$fake_bin/tailscale"
export SENTINAL_TEST_CURL_LOG="$test_root/curl.log"

run() {
  local dir="$1"
  shift
  SENTINAL_TEST_RELEASE_DIR="$dir" PATH="$fake_bin:/usr/bin:/bin:/usr/sbin:/sbin" HOME="$test_root/home" \
    sh "$release/install.sh" --no-service "$@"
}

# 1. Latest release: verified and installed under ~/.local with a clear next step.
mkdir -p "$test_root/home"
output="$(run "$release" 2>&1)" || fail "a signed release failed to install: $output"
[[ -x "$test_root/home/.local/bin/sentinald" ]] || fail 'sentinald was not installed'
grep -q '"version": "0.0.9"' "$test_root/home/.local/share/sentinal/release.json" || fail 'release metadata not recorded'
[[ "$output" == *"owner signature verified"* ]] || fail "no signature confirmation: $output"
[[ "$output" == *"sentinal setup"* || "$output" == *"docs/SETUP.md"* ]] || fail "no next step: $output"
if grep -v -E '^https://github.com/example/sentinal/releases/(latest/download|download/v0\.0\.9)/' "$SENTINAL_TEST_CURL_LOG" | grep -q .; then
  fail 'downloaded from somewhere other than the GitHub release'
fi

# 2. A pinned version is fetched from that exact tag.
: >"$SENTINAL_TEST_CURL_LOG"
run "$release" --version 0.0.9 >/dev/null 2>&1 || fail '--version 0.0.9 failed'
grep -q '/releases/download/v0.0.9/SHA256SUMS.sig$' "$SENTINAL_TEST_CURL_LOG" || fail '--version did not use the exact tag'

# 3. Tampered SHA256SUMS, a missing signature, or an unpinned key installs nothing.
for case in tampered unsigned stranger; do
  bad="$test_root/bad $case"
  rm -rf "$bad" "$test_root/home"
  mkdir -p "$test_root/home"
  cp -R "$release" "$bad"
  case "$case" in
    tampered) printf '%s  extra.tar.gz\n' "$(sha256 "$bad/install.sh")" >>"$bad/SHA256SUMS" ;;
    unsigned) rm -f "$bad/SHA256SUMS.sig" ;;
    stranger) rm -f "$bad/SHA256SUMS.sig"; ssh-keygen -q -Y sign -n sentinal-release -f "$test_root/stranger-key" "$bad/SHA256SUMS" ;;
  esac
  if error="$(run "$bad" 2>&1)"; then
    fail "$case release was installed"
  fi
  [[ "$error" == *"not signed"* ]] || fail "unclear $case error: $error"
  [[ ! -e "$test_root/home/.local/bin/sentinald" ]] || fail "$case release left programs installed"
done

# 4. An unsupported platform gets an honest refusal before any download.
: >"$SENTINAL_TEST_CURL_LOG"
mkdir -p "$test_root/uname"
printf '#!/bin/sh\ncase "$1" in -s) echo FreeBSD ;; -m) echo amd64 ;; esac\n' >"$test_root/uname/uname"
chmod 755 "$test_root/uname/uname"
if error="$(SENTINAL_TEST_RELEASE_DIR="$release" PATH="$test_root/uname:$fake_bin:/usr/bin:/bin" HOME="$test_root/home" sh "$release/install.sh" 2>&1)"; then
  fail 'installed on an unsupported platform'
fi
[[ "$error" == *"unsupported target 'FreeBSD:amd64'"* ]] || fail "unclear unsupported error: $error"
[[ ! -s "$SENTINAL_TEST_CURL_LOG" ]] || fail 'downloaded before refusing an unsupported platform'

echo 'github install test: passed'
