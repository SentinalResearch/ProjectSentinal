#!/usr/bin/env bash

sentinal_package_die() {
  echo "sentinal package: $*" >&2
  exit 1
}

sentinal_package_valid_version() {
  local value="$1" major minor patch extra part
  IFS=. read -r major minor patch extra <<<"$value"
  [[ -z "${extra:-}" && -n "${major:-}" && -n "${minor:-}" && -n "${patch:-}" ]] || return 1
  for part in "$major" "$minor" "$patch"; do
    [[ "$part" =~ ^(0|[1-9][0-9]*)$ ]] || return 1
  done
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
