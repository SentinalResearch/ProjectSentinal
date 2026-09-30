#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/package-common.sh"

usage() {
  cat <<'EOF'
Usage:
  scripts/publish-worker-release.sh publish --version VERSION --package-dir DIR --artifact FILE [--artifact FILE...] [--published-at MS]
  scripts/publish-worker-release.sh rollback --version VERSION --package-dir DIR
EOF
}

action="${1:-}"
[[ -n "$action" ]] || { usage >&2; exit 2; }
shift
version=""
package_dir=""
published_at=""
artifacts=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --package-dir) package_dir="${2:-}"; shift 2 ;;
    --published-at) published_at="${2:-}"; shift 2 ;;
    --artifact) artifacts+=("${2:-}"); shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) sentinal_package_die "unknown publish argument '$1'" ;;
  esac
done

sentinal_package_valid_version "$version" || sentinal_package_die "invalid release version '$version'; expected MAJOR.MINOR.PATCH"
[[ -n "$package_dir" ]] || sentinal_package_die "--package-dir is required"
sentinal_package_require_absolute package "$package_dir"
mkdir -p "$package_dir/channels" "$package_dir/releases" "$package_dir/artifacts" "$package_dir/.staging"
chmod 700 "$package_dir" "$package_dir/channels" "$package_dir/releases" "$package_dir/artifacts" "$package_dir/.staging"

if [[ "$action" == rollback ]]; then
  [[ ${#artifacts[@]} -eq 0 ]] || sentinal_package_die "rollback does not accept artifacts"
  release="$package_dir/releases/$version.json"
  [[ -f "$release" && ! -L "$release" ]] || sentinal_package_die "cannot roll back: release '$version' is not published"
  temporary="$package_dir/channels/stable.json.tmp.$$"
  trap 'rm -f -- "$temporary"' EXIT HUP INT TERM
  cp "$release" "$temporary"
  chmod 600 "$temporary"
  mv -f "$temporary" "$package_dir/channels/stable.json"
  echo "Rolled stable back to $version"
  exit 0
fi
[[ "$action" == publish ]] || sentinal_package_die "unknown action '$action'"
[[ ${#artifacts[@]} -ge 1 && ${#artifacts[@]} -le 16 ]] || sentinal_package_die "publish requires 1..16 --artifact values"
if [[ -z "$published_at" ]]; then
  published_at="$(($(date +%s) * 1000))"
fi
[[ "$published_at" =~ ^(0|[1-9][0-9]*)$ ]] || sentinal_package_die "invalid published time '$published_at'"

mapfile_supported=true
if ! command -v mapfile >/dev/null 2>&1; then mapfile_supported=false; fi
if [[ "$mapfile_supported" == true ]]; then
  mapfile -t sorted_artifacts < <(printf '%s\n' "${artifacts[@]}" | LC_ALL=C sort)
else
  sorted_artifacts=()
  while IFS= read -r value; do sorted_artifacts+=("$value"); done < <(printf '%s\n' "${artifacts[@]}" | LC_ALL=C sort)
fi

stage="$package_dir/.staging/publish-$version-$$"
cleanup() { rm -rf -- "$stage"; }
trap cleanup EXIT HUP INT TERM
mkdir -p "$stage/artifacts"
manifest="$stage/manifest.json"
{
  printf '{\n'
  printf '  "schema_version": 1,\n'
  printf '  "channel": "stable",\n'
  printf '  "release_version": "%s",\n' "$version"
  printf '  "published_at": %s,\n' "$published_at"
  printf '  "minimum_protocol_version": 1,\n'
  printf '  "maximum_protocol_version": 1,\n'
  printf '  "targets": [\n'
  index=0
  seen_targets=' '
  for artifact in "${sorted_artifacts[@]}"; do
    sentinal_package_require_absolute artifact "$artifact"
    [[ -f "$artifact" && ! -L "$artifact" ]] || sentinal_package_die "artifact '$artifact' is not a regular file"
    name="$(basename "$artifact")"
    prefix="sentinal-worker-$version-"
    [[ "$name" == "$prefix"*.tar.gz ]] || sentinal_package_die "artifact '$name' does not match release '$version'"
    target="${name#$prefix}"
    target="${target%.tar.gz}"
    read -r os arch <<<"$(sentinal_package_target_values "$target")"
    case "$seen_targets" in *" $target "*) sentinal_package_die "duplicate target '$target'" ;; esac
    seen_targets="$seen_targets$target "
    bytes="$(sentinal_package_length "$artifact")"
    ((bytes > 0 && bytes <= 268435456)) || sentinal_package_die "artifact '$name' length $bytes exceeds 1..=268435456"
    digest="$(sentinal_package_sha256 "$artifact")"
    ((index == 0)) || printf ',\n'
    cat <<EOF
    {
      "target": "$target",
      "os": "$os",
      "arch": "$arch",
      "artifact_path": "/packages/v1/artifacts/$name",
      "bytes": $bytes,
      "sha256": "$digest"
    }
EOF
    cp "$artifact" "$stage/artifacts/$name"
    chmod 600 "$stage/artifacts/$name"
    index=$((index + 1))
  done
  printf '  ]\n}\n'
} >"$manifest"
chmod 600 "$manifest"
(( $(sentinal_package_length "$manifest") <= 65536 )) || sentinal_package_die "release manifest exceeds 65536 bytes"

release="$package_dir/releases/$version.json"
if [[ -e "$release" ]]; then
  [[ -f "$release" && ! -L "$release" ]] || sentinal_package_die "published release '$release' is unsafe"
  cmp -s "$manifest" "$release" || sentinal_package_die "release '$version' already exists with different content"
fi

for staged in "$stage/artifacts"/*; do
  name="$(basename "$staged")"
  destination="$package_dir/artifacts/$name"
  if [[ -e "$destination" ]]; then
    cmp -s "$staged" "$destination" || sentinal_package_die "artifact '$name' already exists with different content"
  else
    mv "$staged" "$destination"
  fi
done
if [[ ! -e "$release" ]]; then
  temporary_release="$package_dir/releases/$version.json.tmp.$$"
  cp "$manifest" "$temporary_release"
  chmod 600 "$temporary_release"
  mv "$temporary_release" "$release"
fi

if [[ "${SENTINAL_PUBLISH_FAIL_BEFORE_CHANNEL:-}" == 1 ]]; then
  sentinal_package_die "injected interruption before channel update"
fi
temporary_channel="$package_dir/channels/stable.json.tmp.$$"
cp "$release" "$temporary_channel"
chmod 600 "$temporary_channel"
mv -f "$temporary_channel" "$package_dir/channels/stable.json"
echo "Published Sentinal worker $version to stable"
