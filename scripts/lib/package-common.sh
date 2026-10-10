#!/usr/bin/env bash

sentinal_package_die() {
  echo "sentinal package: $*" >&2
  exit 1
}

sentinal_package_valid_version() {
  # The whole string must match; splitting on "." let "0.1.17." through.
  [[ "$1" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]
}

sentinal_package_target_values() {
  case "$1" in
    x86_64-unknown-linux-musl) printf 'LINUX X86_64\n' ;;
    aarch64-unknown-linux-musl) printf 'LINUX AARCH64\n' ;;
    x86_64-apple-darwin) printf 'MACOS X86_64\n' ;;
    aarch64-apple-darwin) printf 'MACOS AARCH64\n' ;;
    *) sentinal_package_die "unsupported target '$1'" ;;
  esac
}

sentinal_package_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sentinal_package_die "sha256sum or shasum is required"
  fi
}

sentinal_package_length() {
  wc -c <"$1" | tr -d '[:space:]'
}

sentinal_package_require_absolute() {
  local label="$1" value="$2"
  [[ "$value" == /* ]] || sentinal_package_die "$label path '$value' must be absolute"
  case "$value" in
    /|*/../*|*/..|*/./*) sentinal_package_die "refusing unsafe $label path '$value'" ;;
  esac
}
