#!/usr/bin/env bash
# Fetch, before a release build, everything the daemon's terminal engine
# (libghostty-vt, via the ghostty-vt crate) needs, so the build itself runs
# offline. Every file is pinned by sha256 here; the crate checks the Ghostty
# files again against its own GHOSTTY.lock.
#
#   scripts/fetch-terminal-engine.sh TARGET DIR
#
# Prints the environment to export (GHOSTTY_RELEASE_DIR, and ZIG on Linux)
# as KEY=VALUE lines, for $GITHUB_ENV.
#
# macOS: the prebuilt XCFramework; no Zig.
# Linux: the source and its dependencies tarballs, and Zig 0.17.1 to build them.
set -euo pipefail

readonly GHOSTTY_RELEASE="https://release.files.ghostty.org/libghostty-vt/1.0.1"
readonly GHOSTTY_SOURCE="libghostty-vt-1.0.1.tar.gz 333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa"
readonly GHOSTTY_DEPS="libghostty-vt-1.0.1-deps.tar.gz 91a4e10960a52bf5bf4b15aa2a2cddaff8a89d6cd903d3055bda4fd17de3356d"
readonly GHOSTTY_XCFRAMEWORK="ghostty-vt-1.0.1.xcframework.zip c0f9631277f726154c8f521954d17cdc41aac4626fc0df8cf925a19ee58e829b"
# From https://ziglang.org/download/index.json, release 0.17.1.
readonly ZIG_VERSION="0.17.1"
readonly ZIG_X86_64="zig-x86_64-linux-0.17.1.tar.xz 3581863e3cbcfd0fce0efc682ce7e9e93da96668a977791d356a26d2a7e9b17a"
readonly ZIG_AARCH64="zig-aarch64-linux-0.17.1.tar.xz a1546d5646eb184437918d6635ca6b762a0e889eac09cef720ee7a1ed3fe7795"

die() {
  echo "fetch-terminal-engine: $*" >&2
  exit 1
}

[[ $# -eq 2 ]] || die "usage: $0 TARGET DIR"
target="$1"
dir="$2"
mkdir -p "$dir/ghostty"
dir="$(cd "$dir" && pwd)"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# fetch URL_BASE "NAME SHA256" DEST_DIR
fetch() {
  local base="$1" name="${2% *}" sha="${2#* }" dest="$3"
  local file="$dest/$name"
  if [[ ! -f "$file" || "$(sha256_of "$file")" != "$sha" ]]; then
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
      --retry 3 --output "$file.part" "$base/$name"
    mv "$file.part" "$file"
  fi
  [[ "$(sha256_of "$file")" == "$sha" ]] || die "$name does not match its pinned sha256"
}

case "$target" in
  *-apple-darwin)
    fetch "$GHOSTTY_RELEASE" "$GHOSTTY_XCFRAMEWORK" "$dir/ghostty"
    echo "GHOSTTY_RELEASE_DIR=$dir/ghostty"
    ;;
  *-unknown-linux-*)
    fetch "$GHOSTTY_RELEASE" "$GHOSTTY_SOURCE" "$dir/ghostty"
    fetch "$GHOSTTY_RELEASE" "$GHOSTTY_DEPS" "$dir/ghostty"
    # Zig runs on the build machine, whatever the target.
    case "$(uname -m)" in
      x86_64) zig="$ZIG_X86_64" ;;
      aarch64 | arm64) zig="$ZIG_AARCH64" ;;
      *) die "no pinned Zig for this build machine: $(uname -m)" ;;
    esac
    fetch "https://ziglang.org/download/$ZIG_VERSION" "$zig" "$dir"
    name="${zig% *}"
    tar -xJf "$dir/$name" -C "$dir"
    echo "GHOSTTY_RELEASE_DIR=$dir/ghostty"
    echo "ZIG=$dir/${name%.tar.xz}/zig"
    ;;
  *) die "no terminal engine files for $target" ;;
esac
