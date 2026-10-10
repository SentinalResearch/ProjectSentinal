#!/usr/bin/env bash

# Shared validation and rendering for the development launch roles.

sentinal_die() {
  echo "sentinal ${SENTINAL_LAUNCH_ROLE:-launcher}: $*" >&2
  exit 1
}

sentinal_require_command() {
  command -v "$1" >/dev/null 2>&1 || sentinal_die "required command '$1' was not found"
}

sentinal_validate_port() {
  local name="$1"
  local value="$2"
  if [[ ! "$value" =~ ^(0|[1-9][0-9]{0,4})$ ]] || ((value < 1 || value > 65535)); then
    sentinal_die "invalid $name port '$value'; expected 1..65535"
  fi
}

sentinal_validate_tailscale_ipv4() {
  local value="$1"
  local first second third fourth extra octet
  IFS=. read -r first second third fourth extra <<<"$value"
  # Splitting alone accepts a trailing "." (empty extra); require the exact shape.
  if [[ ! "$value" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ || -n "${extra:-}" || -z "${fourth:-}" ]]; then
    sentinal_die "'$value' is not a Tailscale IPv4 address in 100.64.0.0/10"
  fi
  for octet in "$first" "$second" "$third" "$fourth"; do
    if [[ ! "$octet" =~ ^(0|[1-9][0-9]{0,2})$ ]] || ((octet > 255)); then
      sentinal_die "'$value' is not a Tailscale IPv4 address in 100.64.0.0/10"
    fi
  done
  if ((first != 100 || second < 64 || second > 127)); then
    sentinal_die "'$value' is not a Tailscale IPv4 address in 100.64.0.0/10"
  fi
}

sentinal_require_tailscale_running() {
  sentinal_require_command tailscale
  local status first_line
  if ! status="$(tailscale status 2>&1)"; then
    first_line="${status%%$'\n'*}"
    case "$first_line" in
      "Tailscale is stopped."*|"Tailscale is starting."*|"Logged out."*)
        sentinal_die "Tailscale is not running (status: $first_line). Turn Tailscale on, wait until connected, then try again."
        ;;
      *) sentinal_die "could not read Tailscale status; open Tailscale and confirm it is connected" ;;
    esac
  fi
  first_line="${status%%$'\n'*}"
  case "$first_line" in
    "Tailscale is stopped."*|"Tailscale is starting."*|"Logged out."*)
      sentinal_die "Tailscale is not running (status: $first_line). Turn Tailscale on, wait until connected, then try again."
      ;;
  esac
}

sentinal_discover_tailscale_ipv4() {
  if [[ -n "${SENTINAL_TAILSCALE_IP:-}" ]]; then
    printf '%s\n' "$SENTINAL_TAILSCALE_IP"
    return
  fi
  sentinal_require_command tailscale
  local addresses address
  addresses="$(tailscale ip -4)" || sentinal_die "could not read a Tailscale IPv4 address; is Tailscale connected?"
  while IFS= read -r address; do
    if [[ -n "$address" ]]; then
      printf '%s\n' "$address"
      return
    fi
  done <<<"$addresses"
  sentinal_die "Tailscale reported no IPv4 address; connect Tailscale and try again"
}

sentinal_require_assigned_address() {
  local expected="$1"
  local addresses address
  addresses="$(tailscale ip -4)" || sentinal_die "could not verify the local Tailscale IPv4 address"
  while IFS= read -r address; do
    [[ "$address" != "$expected" ]] || return
  done <<<"$addresses"
  sentinal_die "Tailscale does not currently own '$expected'; available IPv4 addresses: ${addresses//$'\n'/, }"
}

sentinal_resolve_local_ip() {
  local dry_run="$1"
  if [[ "$dry_run" == false || -z "${SENTINAL_TAILSCALE_IP:-}" ]]; then
    sentinal_require_tailscale_running
  fi
  local address
  address="$(sentinal_discover_tailscale_ipv4)"
  sentinal_validate_tailscale_ipv4 "$address"
  [[ "$dry_run" == true ]] || sentinal_require_assigned_address "$address"
  printf '%s\n' "$address"
}

sentinal_toml_string() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  value="${value//$'\r'/\\r}"
  value="${value//$'\t'/\\t}"
  printf '"%s"' "$value"
}

# Extra arguments after the sixth are written verbatim as additional TOML lines.
sentinal_write_daemon_config() {
  local file="$1" bind="$2" data_dir="$3" token_file="$4" auth_file="$5" local_socket="$6"
  shift 6
  {
    printf 'bind = %s\n' "$(sentinal_toml_string "$bind")"
    printf 'data_dir = %s\n' "$(sentinal_toml_string "$data_dir")"
    printf 'scrollback_lines = 10000\n'
    printf 'token_file = %s\n' "$(sentinal_toml_string "$token_file")"
    printf 'auth_file = %s\n' "$(sentinal_toml_string "$auth_file")"
    printf 'local_socket = %s\n' "$(sentinal_toml_string "$local_socket")"
    local line
    for line in "$@"; do
      printf '%s\n' "$line"
    done
  } >"$file"
}

sentinal_require_port_available() {
  local address="$1" port="$2"
  if command -v lsof >/dev/null 2>&1; then
    if lsof -nP "-iTCP@$address:$port" -sTCP:LISTEN >/dev/null 2>&1; then
      sentinal_die "$address:$port is already in use"
    fi
    return 0
  fi
  if command -v ss >/dev/null 2>&1; then
    if ss -H -ltn 2>/dev/null | awk -v endpoint="$address:$port" '$4 == endpoint { found = 1 } END { exit !found }'; then
      sentinal_die "$address:$port is already in use"
    fi
    return 0
  fi
  sentinal_die "cannot verify that $address:$port is free; install lsof or ss"
}

sentinal_resolve_vite_bin() {
  local repo_root="$1"
  local vite_bin="${SENTINAL_VITE_BIN:-$repo_root/web/node_modules/.bin/vite}"
  [[ -x "$vite_bin" ]] || sentinal_die "Vite is not installed; run 'pnpm -C web install' and try again"
  printf '%s\n' "$vite_bin"
}
