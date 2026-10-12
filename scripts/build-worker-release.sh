#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$SCRIPT_DIR/lib/package-common.sh"
source "$SCRIPT_DIR/lib/install-common.sh"

usage() {
  cat <<'EOF'
Usage: scripts/build-worker-release.sh --version VERSION --target TARGET --output DIR [OPTIONS]

Options:
  --source-dir DIR         workspace to build/package (default: repository root)
  --bin-dir DIR            use prebuilt sentinal + sentinald binaries (tests/releases)
  --source-date-epoch SEC  build timestamp metadata (default: SOURCE_DATE_EPOCH or git time)
EOF
}

version=""
target=""
output=""
source_dir="$REPO_ROOT"
bin_dir=""
source_date_epoch="${SOURCE_DATE_EPOCH:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --target) target="${2:-}"; shift 2 ;;
    --output) output="${2:-}"; shift 2 ;;
    --source-dir) source_dir="${2:-}"; shift 2 ;;
    --bin-dir) bin_dir="${2:-}"; shift 2 ;;
    --source-date-epoch) source_date_epoch="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) sentinal_package_die "unknown build argument '$1'" ;;
  esac
done

sentinal_package_valid_version "$version" || sentinal_package_die "invalid release version '$version'; expected MAJOR.MINOR.PATCH"
sentinal_package_target_values "$target" >/dev/null
[[ -n "$output" ]] || sentinal_package_die "--output is required"
sentinal_package_require_absolute output "$output"
sentinal_package_require_absolute source "$source_dir"
[[ -d "$source_dir" ]] || sentinal_package_die "source directory '$source_dir' does not exist"

if [[ -z "$source_date_epoch" ]]; then
  source_date_epoch="$(git -C "$source_dir" log -1 --format=%ct 2>/dev/null || printf '0')"
fi
[[ "$source_date_epoch" =~ ^(0|[1-9][0-9]*)$ ]] || sentinal_package_die "invalid source date epoch '$source_date_epoch'"

if [[ -z "$bin_dir" ]]; then
  command -v cargo >/dev/null 2>&1 || sentinal_package_die "cargo is required to build worker binaries"
  # Stamp the release into the binaries so `sentinal --version` and the
  # registry report it instead of the unchanging Cargo package version.
  (cd "$source_dir" && SENTINAL_RELEASE_VERSION="$version" \
    SENTINAL_GIT_COMMIT="$(git -C "$source_dir" rev-parse --verify HEAD 2>/dev/null || printf 'unknown')" \
    cargo build --locked --offline --release --target "$target" \
    -p daemon --bin sentinald -p cli --bin sentinal)
  bin_dir="$source_dir/target/$target/release"
  # Binaries this script just compiled inherit the caller's umask (often 0002
  # on Ubuntu). Normalise them; supplied --bin-dir binaries are still checked.
  chmod go-w "$bin_dir/sentinal" "$bin_dir/sentinald"
else
  sentinal_package_require_absolute binary "$bin_dir"
fi
sentinal_validate_source_binary "$bin_dir/sentinal"
sentinal_validate_source_binary "$bin_dir/sentinald"

for required in LICENSE scripts/install-worker.sh scripts/lib/install-common.sh scripts/lib/launch-common.sh \
  scripts/receive-release.sh scripts/publish-worker-release.sh scripts/apply-release.sh \
  scripts/lib/package-common.sh scripts/lib/release-signature.sh packaging/release-keys/allowed_signers \
  packaging/systemd/sentinald.service packaging/launchd/dev.sentinal.sentinald.plist \
  web/dist/index.html web/dist/manifest.webmanifest web/dist/sw.js; do
  [[ -f "$source_dir/$required" && ! -L "$source_dir/$required" ]] || sentinal_package_die "missing release asset '$source_dir/$required'"
done
for required in ta.zsh ta.bash ta.fish external/sentinal.zsh external/sentinal.bash external/sentinal.fish vendor/bash-preexec.sh; do
  [[ -f "$source_dir/shell/$required" && ! -L "$source_dir/shell/$required" ]] || sentinal_package_die "missing shell asset '$source_dir/shell/$required'"
done

mkdir -p "$output"
stage="$output/.worker-stage.$$"
archive="$output/sentinal-worker-$version-$target.tar.gz"
temporary_archive="$archive.tmp.$$"
cleanup() {
  rm -rf -- "$stage"
  rm -f -- "$temporary_archive"
}
trap cleanup EXIT HUP INT TERM

root="$stage/sentinal-worker"
mkdir -p "$root/bin" "$root/scripts/lib" "$root/packaging/systemd" "$root/packaging/launchd"
cp "$bin_dir/sentinal" "$root/bin/sentinal"
cp "$bin_dir/sentinald" "$root/bin/sentinald"
chmod 755 "$root/bin/sentinal" "$root/bin/sentinald"
cp -R "$source_dir/shell" "$root/shell"
# Release tools, laid out like the repository so their default paths resolve.
mkdir -p "$root/tools/scripts/lib" "$root/tools/packaging/release-keys"
for tool in receive-release.sh publish-worker-release.sh apply-release.sh; do
  cp "$source_dir/scripts/$tool" "$root/tools/scripts/$tool"
  chmod 755 "$root/tools/scripts/$tool"
done
cp "$source_dir/scripts/lib/package-common.sh" "$source_dir/scripts/lib/release-signature.sh" "$root/tools/scripts/lib/"
cp "$source_dir/packaging/release-keys/allowed_signers" "$root/tools/packaging/release-keys/allowed_signers"
cp -R "$source_dir/web/dist" "$root/web"
web_cache_id="$version-$target"
sed "s/__SENTINAL_RELEASE_ID__/$web_cache_id/g" "$root/web/sw.js" >"$root/web/sw.js.tmp"
mv "$root/web/sw.js.tmp" "$root/web/sw.js"
cp "$source_dir/scripts/install-worker.sh" "$root/scripts/install-worker.sh"
cp "$source_dir/scripts/lib/install-common.sh" "$root/scripts/lib/install-common.sh"
cp "$source_dir/scripts/lib/launch-common.sh" "$root/scripts/lib/launch-common.sh"
chmod 755 "$root/scripts/install-worker.sh"
cp "$source_dir/packaging/systemd/sentinald.service" "$root/packaging/systemd/sentinald.service"
cp "$source_dir/packaging/launchd/dev.sentinal.sentinald.plist" "$root/packaging/launchd/dev.sentinal.sentinald.plist"
cp "$source_dir/LICENSE" "$root/LICENSE"

git_commit="$(git -C "$source_dir" rev-parse --verify HEAD 2>/dev/null || printf 'unknown')"
case "$git_commit" in
  unknown|[0-9a-f][0-9a-f]*) ;;
  *) sentinal_package_die "unsafe git commit metadata '$git_commit'" ;;
esac
cat >"$root/BUILD.json" <<EOF
{
  "schema_version": 1,
  "version": "$version",
  "git_commit": "$git_commit",
  "source_date_epoch": $source_date_epoch,
  "target": "$target",
  "protocol_version": 1
}
EOF

# Normalize permissions and timestamps; gzip -n removes its own timestamp.
# Sorted, non-recursive input and numeric ownership make the archive repeatable
# without duplicate hardlink entries when directories are listed explicitly.
find "$root" -type d -exec chmod 755 {} +
find "$root" -type f -exec chmod 644 {} +
chmod 755 "$root/bin/sentinal" "$root/bin/sentinald" "$root/scripts/install-worker.sh"
find "$root" -exec touch -t 198001010000 {} +
(cd "$stage" && find sentinal-worker -print | LC_ALL=C sort >"$stage/entries")
if tar --version 2>/dev/null | grep -qi 'gnu tar'; then
  (cd "$stage" && tar --no-recursion --format=ustar --owner=0 --group=0 --numeric-owner -cf - -T "$stage/entries") | gzip -n >"$temporary_archive"
else
  (export COPYFILE_DISABLE=1; cd "$stage" && tar --no-recursion --format ustar --uid 0 --gid 0 --numeric-owner -cf - -T "$stage/entries") | gzip -n >"$temporary_archive"
fi
chmod 600 "$temporary_archive"
mv -f "$temporary_archive" "$archive"
printf '%s\n' "$archive"
