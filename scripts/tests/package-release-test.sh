#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
builder="$repo_root/scripts/build-worker-release.sh"
publisher="$repo_root/scripts/publish-worker-release.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-package-release-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "package release test failed: $*" >&2
  exit 1
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# Throwaway release keys: one pinned, one not.
ssh-keygen -q -t ed25519 -N "" -C pinned -f "$test_root/release-key"
ssh-keygen -q -t ed25519 -N "" -C stranger -f "$test_root/stranger-key"
allowed="$test_root/allowed_signers"
printf 'sentinal-release namespaces="sentinal-release" %s\n' "$(cut -d' ' -f1,2 "$test_root/release-key.pub")" >"$allowed"

# Write SHA256SUMS for DIR's archives and sign it with KEY (default: pinned).
sign_dir() {
  local dir="$1" key="${2:-$test_root/release-key}"
  (cd "$dir" && for file in *.tar.gz; do printf '%s  %s\n' "$(sha256 "$file")" "$file"; done) >"$dir/SHA256SUMS"
  rm -f "$dir/SHA256SUMS.sig"
  ssh-keygen -q -Y sign -n sentinal-release -f "$key" "$dir/SHA256SUMS"
}

make_fake_bins() {
  local root="$1" marker="$2"
  mkdir -p "$root"
  printf '#!/bin/sh\necho sentinal-%s\n' "$marker" >"$root/sentinal"
  printf '#!/bin/sh\necho sentinald-%s\n' "$marker" >"$root/sentinald"
  chmod 755 "$root/sentinal" "$root/sentinald"
}

fake_bins="$test_root/fake bins"
make_fake_bins "$fake_bins" one
first="$test_root/build one"
second="$test_root/build two"

"$builder" --version 0.0.1 --target x86_64-apple-darwin \
  --bin-dir "$fake_bins" --output "$first" --source-date-epoch 1790712000 >/dev/null
"$builder" --version 0.0.1 --target x86_64-apple-darwin \
  --bin-dir "$fake_bins" --output "$second" --source-date-epoch 1790712000 >/dev/null

artifact_name='sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz'
first_artifact="$first/$artifact_name"
second_artifact="$second/$artifact_name"
[[ -f "$first_artifact" ]] || fail 'builder did not create the expected artifact'
[[ "$(sha256 "$first_artifact")" == "$(sha256 "$second_artifact")" ]] || fail 'same inputs did not produce the same archive'

entries="$(tar -tzf "$first_artifact")"
duplicates="$(printf '%s\n' "$entries" | LC_ALL=C sort | uniq -d)"
[[ -z "$duplicates" ]] || fail "archive contains duplicate entries: $duplicates"
while IFS= read -r listing; do
  permissions="${listing%% *}"
  case "${permissions:0:1}" in
    -|d) ;;
    *) fail "archive contains a non-file entry: $listing" ;;
  esac
  [[ "${permissions:5:1}" != w && "${permissions:8:1}" != w ]] || \
    fail "archive entry is group/world writable: $listing"
done < <(tar -tvzf "$first_artifact")
for required in \
  sentinal-worker/BUILD.json \
  sentinal-worker/bin/sentinal \
  sentinal-worker/bin/sentinald \
  sentinal-worker/web/index.html \
  sentinal-worker/web/sw.js \
  sentinal-worker/scripts/install-worker.sh \
  sentinal-worker/shell/external/sentinal.zsh \
  sentinal-worker/packaging/systemd/sentinald.service; do
  grep -Fxq "$required" <<<"$entries" || fail "archive is missing $required"
done
service_worker="$(tar -xOzf "$first_artifact" sentinal-worker/web/sw.js)"
grep -Fq 'sentinal-0.0.1-x86_64-apple-darwin' <<<"$service_worker" || fail 'service worker cache is not release-specific'
if grep -Fq '__SENTINAL_RELEASE_ID__' <<<"$service_worker"; then
  fail 'service worker retained its release marker'
fi

if "$builder" --version 0.0.1 --target riscv64-unknown-linux-musl \
  --bin-dir "$fake_bins" --output "$test_root/unsupported" 2>"$test_root/error"; then
  fail 'unsupported target was accepted'
fi
grep -Fq "unsupported target 'riscv64-unknown-linux-musl'" "$test_root/error" || fail 'unsupported target error was unclear'

package_dir="$test_root/package service"
first_dir="$(dirname "$first_artifact")"
sign_dir "$first_dir"

# Unsigned, unpinned, and mismatched publishes are refused before anything moves.
if error="$("$publisher" publish --version 0.0.1 --package-dir "$package_dir" \
  --published-at 1790712000000 --artifact "$first_artifact" 2>&1)"; then
  fail 'an unsigned release was published'
fi
[[ "$error" == *"--sums and --signature are required"* ]] || fail "unclear unsigned error: $error"
stranger_dir="$test_root/stranger"
mkdir -p "$stranger_dir" && cp "$first_artifact" "$stranger_dir/"
sign_dir "$stranger_dir" "$test_root/stranger-key"
if error="$("$publisher" publish --version 0.0.1 --package-dir "$package_dir" \
  --published-at 1790712000000 --artifact "$first_artifact" --sums "$stranger_dir/SHA256SUMS" --signature "$stranger_dir/SHA256SUMS.sig" --allowed-signers "$allowed" 2>&1)"; then
  fail 'a release signed by an unpinned key was published'
fi
[[ "$error" == *"not signed by a trusted Sentinal release key"* ]] || fail "unclear unpinned error: $error"
[[ ! -e "$package_dir/channels/stable.json" ]] || fail 'a refused publish moved stable'

"$publisher" publish --version 0.0.1 --package-dir "$package_dir" \
  --published-at 1790712000000 --artifact "$first_artifact" --sums "$first_dir/SHA256SUMS" --signature "$first_dir/SHA256SUMS.sig" --allowed-signers "$allowed" >/dev/null
cmp -s "$first_dir/SHA256SUMS" "$package_dir/releases/0.0.1.SHA256SUMS" || fail 'signed SHA256SUMS was not stored with the release'
cmp -s "$first_dir/SHA256SUMS.sig" "$package_dir/releases/0.0.1.SHA256SUMS.sig" || fail 'signature was not stored with the release'

stable="$package_dir/channels/stable.json"
release="$package_dir/releases/0.0.1.json"
published="$package_dir/artifacts/$artifact_name"
[[ -f "$stable" && -f "$release" && -f "$published" ]] || fail 'publisher omitted package files'
cmp -s "$stable" "$release" || fail 'stable pointer does not contain the published release manifest'
grep -Fq '"minimum_protocol_version": 1' "$release" || fail 'manifest omitted protocol compatibility'
grep -Fq "\"sha256\": \"$(sha256 "$first_artifact")\"" "$release" || fail 'manifest checksum is incorrect'
grep -Fq "\"bytes\": $(wc -c <"$first_artifact" | tr -d '[:space:]')" "$release" || fail 'manifest length is incorrect'

# Publishing identical bytes is idempotent.
"$publisher" publish --version 0.0.1 --package-dir "$package_dir" \
  --published-at 1790712000000 --artifact "$first_artifact" --sums "$first_dir/SHA256SUMS" --signature "$first_dir/SHA256SUMS.sig" --allowed-signers "$allowed" >/dev/null

make_fake_bins "$fake_bins" changed
changed_dir="$test_root/changed"
"$builder" --version 0.0.1 --target x86_64-apple-darwin \
  --bin-dir "$fake_bins" --output "$changed_dir" --source-date-epoch 1790712000 >/dev/null
if error="$("$publisher" publish --version 0.0.1 --package-dir "$package_dir" \
  --published-at 1790712000000 --artifact "$changed_dir/$artifact_name" --sums "$first_dir/SHA256SUMS" --signature "$first_dir/SHA256SUMS.sig" --allowed-signers "$allowed" 2>&1)"; then
  fail 'an artifact that does not match the signed SHA256SUMS was accepted'
fi
[[ "$error" == *"does not match the signed SHA256SUMS"* ]] || fail "unclear signed-mismatch error: $error"
sign_dir "$changed_dir"
if "$publisher" publish --version 0.0.1 --package-dir "$package_dir" \
  --published-at 1790712000000 --artifact "$changed_dir/$artifact_name" --sums "$changed_dir/SHA256SUMS" --signature "$changed_dir/SHA256SUMS.sig" --allowed-signers "$allowed" 2>"$test_root/conflict"; then
  fail 'same version with different content was accepted'
fi
grep -Fq "release '0.0.1' already exists with different content" "$test_root/conflict" || fail 'release conflict error was unclear'

# A failure before the channel switch leaves the previous stable release intact.
v2_dir="$test_root/v2"
"$builder" --version 0.0.2 --target x86_64-apple-darwin \
  --bin-dir "$fake_bins" --output "$v2_dir" --source-date-epoch 1790712001 >/dev/null
sign_dir "$v2_dir"
stable_before="$(sha256 "$stable")"
if SENTINAL_PUBLISH_FAIL_BEFORE_CHANNEL=1 "$publisher" publish --version 0.0.2 \
  --package-dir "$package_dir" --published-at 1790712001000 --sums "$v2_dir/SHA256SUMS" --signature "$v2_dir/SHA256SUMS.sig" --allowed-signers "$allowed" \
  --artifact "$v2_dir/sentinal-worker-0.0.2-x86_64-apple-darwin.tar.gz" 2>"$test_root/interrupted"; then
  fail 'injected publish interruption unexpectedly succeeded'
fi
[[ "$stable_before" == "$(sha256 "$stable")" ]] || fail 'interrupted publish changed stable channel'

"$publisher" publish --version 0.0.2 --package-dir "$package_dir" \
  --published-at 1790712001000 --sums "$v2_dir/SHA256SUMS" --signature "$v2_dir/SHA256SUMS.sig" --allowed-signers "$allowed" \
  --artifact "$v2_dir/sentinal-worker-0.0.2-x86_64-apple-darwin.tar.gz" >/dev/null
grep -Fq '"release_version": "0.0.2"' "$stable" || fail 'stable did not advance to v2'
"$publisher" rollback --version 0.0.1 --package-dir "$package_dir" >/dev/null
grep -Fq '"release_version": "0.0.1"' "$stable" || fail 'rollback did not restore v1 pointer'

if find "$package_dir/.staging" -mindepth 1 -print -quit | grep -q .; then
  fail 'publisher left staging files behind'
fi

# Binaries the builder compiles itself are normalised, even under a umask of
# 0002 (Ubuntu's default with user-private groups). Supplied --bin-dir
# binaries are still refused when group-writable.
source_copy="$test_root/source copy"
mkdir -p "$source_copy/web/dist" "$source_copy/scripts/lib" "$source_copy/packaging"
cp "$repo_root/LICENSE" "$source_copy/"
cp -R "$repo_root/shell" "$source_copy/shell"
cp -R "$repo_root/packaging/systemd" "$repo_root/packaging/launchd" "$source_copy/packaging/"
cp "$repo_root/scripts/install-worker.sh" "$source_copy/scripts/"
cp "$repo_root/scripts/lib/install-common.sh" "$repo_root/scripts/lib/launch-common.sh" "$source_copy/scripts/lib/"
for asset in index.html manifest.webmanifest sw.js; do
  printf 'asset %s\n' "$asset" >"$source_copy/web/dist/$asset"
done
fake_tools="$test_root/fake tools"
mkdir -p "$fake_tools"
cat >"$fake_tools/cargo" <<'EOF'
#!/bin/sh
target=""
while [ "$#" -gt 0 ]; do
  [ "$1" = "--target" ] && target="$2"
  shift
done
mkdir -p "target/$target/release"
printf '%s %s\n' "${SENTINAL_RELEASE_VERSION:-unset}" "${SENTINAL_GIT_COMMIT:-unset}" >"target/stamp"
for name in sentinal sentinald; do
  printf '#!/bin/sh\necho %s\n' "$name" >"target/$target/release/$name"
  chmod 775 "target/$target/release/$name"
done
EOF
chmod 755 "$fake_tools/cargo"
built_dir="$test_root/built by cargo"
(umask 0002 && PATH="$fake_tools:$PATH" "$builder" --version 0.0.3 --target x86_64-apple-darwin \
  --source-dir "$source_copy" --output "$built_dir" --source-date-epoch 1790712002 >/dev/null) ||
  fail 'builder refused binaries it compiled itself under umask 0002'
built_listing="$(tar -tvzf "$built_dir/sentinal-worker-0.0.3-x86_64-apple-darwin.tar.gz" sentinal-worker/bin/sentinald)"
[[ "$built_listing" == -rwxr-xr-x* ]] || fail "built binary mode is not 0755: $built_listing"
read -r stamped_version stamped_commit <"$source_copy/target/stamp"
[[ "$stamped_version" == 0.0.3 ]] || fail "cargo did not receive SENTINAL_RELEASE_VERSION (got '$stamped_version')"
[[ -n "$stamped_commit" && "$stamped_commit" != unset ]] || fail 'cargo did not receive SENTINAL_GIT_COMMIT' 

writable_bins="$test_root/writable bins"
make_fake_bins "$writable_bins" writable
chmod 775 "$writable_bins/sentinal"
if "$builder" --version 0.0.4 --target x86_64-apple-darwin \
  --bin-dir "$writable_bins" --output "$test_root/writable" 2>/dev/null; then
  fail 'builder accepted a group-writable supplied binary'
fi

echo 'package release test: passed'
