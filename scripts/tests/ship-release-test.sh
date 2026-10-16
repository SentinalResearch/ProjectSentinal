#!/usr/bin/env bash
# scripts/ship-release.sh with fake gh and git: tag, wait for the build,
# download the draft, sign with a throwaway pinned key, upload, publish as the
# latest normal release; re-running after success changes nothing.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-ship-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "ship release test failed: $*" >&2
  exit 1
}

ssh-keygen -q -t ed25519 -N "" -C ship -f "$test_root/release-key"
allowed="$test_root/allowed_signers"
printf 'sentinal-release namespaces="sentinal-release" %s\n' "$(cut -d' ' -f1,2 "$test_root/release-key.pub")" >"$allowed"

draft="$test_root/draft"
mkdir -p "$draft"
printf 'archive' >"$draft/sentinal-worker-0.2.2-x86_64-unknown-linux-musl.tar.gz"
printf 'installer' >"$draft/install.sh"
(cd "$draft" && for file in *.tar.gz install.sh; do
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$file"; else shasum -a 256 "$file"; fi
done) | awk '{print $1 "  " $2}' >"$draft/SHA256SUMS"

bin="$test_root/bin"
mkdir -p "$bin"
log="$test_root/calls.log"
state="$test_root/state"
mkdir -p "$state"
cat >"$bin/git" <<FAKE
#!/bin/sh
echo "git \$*" >>"$log"
case "\$1 \$2" in
  "status --porcelain") exit 0 ;;
  "rev-parse --abbrev-ref") echo main ;;
  "rev-parse -q") [ -f "$state/tagged" ] && exit 0 || exit 1 ;;
  "tag -a") touch "$state/tagged" ;;
esac
exit 0
FAKE
cat >"$bin/gh" <<FAKE
#!/bin/sh
echo "gh \$*" >>"$log"
case "\$1 \$2" in
  "run list") echo 4242 ;;
  "run watch") exit 0 ;;
  "release view")
    if [ -f "$state/published" ]; then echo "SHA256SUMS.sig"; echo "draft=false"; else echo "SHA256SUMS"; echo "draft=true"; fi ;;
  "release download")
    shift 2
    while [ "\$#" -gt 0 ]; do [ "\$1" = --dir ] && dir="\$2"; shift; done
    mkdir -p "\$dir" && cp "$draft"/* "\$dir"/ ;;
  "release upload") cp "\$4" "$state/uploaded.sig" ;;
  "release edit") touch "$state/published" ;;
esac
exit 0
FAKE
chmod 755 "$bin/git" "$bin/gh"

out="$test_root/out"
PATH="$bin:$PATH" bash "$repo_root/scripts/ship-release.sh" 0.2.2 --key "$test_root/release-key" \
  --allowed-signers "$allowed" --work-dir "$out" >"$test_root/stdout" 2>&1 ||
  { cat "$test_root/stdout"; fail 'ship-release failed'; }

grep -q 'git tag -a v0.2.2' "$log" || fail 'the tag was not created'
grep -q 'git push origin v0.2.2' "$log" || fail 'the tag was not pushed'
grep -q 'gh run watch 4242 --exit-status' "$log" || fail 'it did not wait for the build'
[[ -f "$state/uploaded.sig" ]] || fail 'no signature was uploaded'
ssh-keygen -Y verify -f "$allowed" -I sentinal-release -n sentinal-release \
  -s "$state/uploaded.sig" <"$draft/SHA256SUMS" >/dev/null 2>&1 || fail 'the uploaded signature does not verify'
grep -q 'gh release edit v0.2.2 --draft=false --prerelease=false --latest' "$log" ||
  fail 'the release was not published as the latest normal release'
grep -q 'released' "$test_root/stdout" || fail "no final summary: $(cat "$test_root/stdout")"

# Re-running after success tags, signs, and uploads nothing again.
: >"$log"
PATH="$bin:$PATH" bash "$repo_root/scripts/ship-release.sh" 0.2.2 --key "$test_root/release-key" \
  --allowed-signers "$allowed" --work-dir "$out" >"$test_root/stdout" 2>&1 || fail 'a re-run failed'
! grep -q 'git tag -a\|gh release upload\|gh release edit' "$log" || fail "a re-run repeated work: $(cat "$log")"
grep -q 'already released' "$test_root/stdout" || fail 're-run did not say it was already released'

# Bad versions and a dirty tree are refused before anything happens.
if PATH="$bin:$PATH" bash "$repo_root/scripts/ship-release.sh" v0.2 --key "$test_root/release-key" >/dev/null 2>&1; then
  fail 'accepted a bad version'
fi
rm -f "$state/published"
cat >"$bin/git" <<FAKE
#!/bin/sh
case "\$1 \$2" in "status --porcelain") echo " M crates/x.rs" ;; "rev-parse -q") exit 1 ;; esac
exit 0
FAKE
if PATH="$bin:$PATH" bash "$repo_root/scripts/ship-release.sh" 0.2.3 --key "$test_root/release-key" \
  --allowed-signers "$allowed" --work-dir "$out" >/dev/null 2>&1; then
  fail 'tagged with uncommitted changes'
fi

echo 'ship release test: passed'
