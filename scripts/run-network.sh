#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

daemon_pid=""
web_pid=""
runtime_dir=""

usage() {
  cat <<'EOF'
Usage: scripts/run-network.sh [--dry-run]

Starts sentinald and the Vite web UI on this machine's Tailscale IPv4
address. Public wildcard and ordinary LAN addresses are refused.

Environment overrides:
  SENTINAL_TAILSCALE_IP  Tailscale IPv4 instead of `tailscale ip -4`
  SENTINAL_DAEMON_PORT   daemon port (default: 7420)
  SENTINAL_WEB_PORT      web port (default: 5173)
  SENTINAL_DATA_DIR      persistent daemon data directory
  SENTINAL_CONFIG_DIR    persistent token/auth directory
EOF
}

die() {
  echo "sentinal network: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command '$1' was not found"
}

validate_port() {
  local name="$1"
  local value="$2"
  if [[ ! "$value" =~ ^(0|[1-9][0-9]{0,4})$ ]] || ((value < 1 || value > 65535)); then
    die "invalid $name port '$value'; expected 1..65535"
  fi
}

validate_tailscale_ipv4() {
  local value="$1"
  local first second third fourth extra
  IFS=. read -r first second third fourth extra <<<"$value"
  if [[ -n "${extra:-}" || -z "${fourth:-}" ]]; then
    die "'$value' is not a Tailscale IPv4 address in 100.64.0.0/10"
  fi
  local octet
  for octet in "$first" "$second" "$third" "$fourth"; do
    if [[ ! "$octet" =~ ^(0|[1-9][0-9]{0,2})$ ]] || ((octet > 255)); then
      die "'$value' is not a Tailscale IPv4 address in 100.64.0.0/10"
    fi
  done
  if ((first != 100 || second < 64 || second > 127)); then
    die "'$value' is not a Tailscale IPv4 address in 100.64.0.0/10"
  fi
}

discover_tailscale_ipv4() {
  if [[ -n "${SENTINAL_TAILSCALE_IP:-}" ]]; then
    printf '%s\n' "$SENTINAL_TAILSCALE_IP"
    return
  fi
  require_command tailscale
  local addresses address
  addresses="$(tailscale ip -4)" || die "could not read a Tailscale IPv4 address; is Tailscale connected?"
  while IFS= read -r address; do
    if [[ -n "$address" ]]; then
      printf '%s\n' "$address"
      return
    fi
  done <<<"$addresses"
  die "Tailscale reported no IPv4 address; connect Tailscale and try again"
}

require_tailscale_running() {
  require_command tailscale
  local status first_line
  if ! status="$(tailscale status 2>&1)"; then
    first_line="${status%%$'\n'*}"
    case "$first_line" in
      "Tailscale is stopped."*|"Tailscale is starting."*|"Logged out."*)
        die "Tailscale is not running (status: $first_line). Turn Tailscale on, wait until connected, then run 'just network' again."
        ;;
      *)
        die "could not read Tailscale status; open Tailscale and confirm it is connected"
        ;;
    esac
  fi
  first_line="${status%%$'\n'*}"
  case "$first_line" in
    "Tailscale is stopped."*|"Tailscale is starting."*|"Logged out."*)
      die "Tailscale is not running (status: $first_line). Turn Tailscale on, wait until connected, then run 'just network' again."
      ;;
  esac
}

toml_string() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  value="${value//$'\r'/\\r}"
  value="${value//$'\t'/\\t}"
  printf '"%s"' "$value"
}

remove_runtime_dir() {
  if [[ -n "$runtime_dir" && -d "$runtime_dir" ]]; then
    case "$runtime_dir" in
      "${TMPDIR:-/tmp}"/sentinal-network.*) rm -rf -- "$runtime_dir" ;;
      *) echo "sentinal network: refusing to remove unexpected runtime path '$runtime_dir'" >&2 ;;
    esac
  fi
}

cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$web_pid" ]]; then
    kill "$web_pid" 2>/dev/null || true
  fi
  if [[ -n "$daemon_pid" ]]; then
    kill "$daemon_pid" 2>/dev/null || true
  fi
  if [[ -n "$web_pid" ]]; then
    wait "$web_pid" 2>/dev/null || true
  fi
  if [[ -n "$daemon_pid" ]]; then
    wait "$daemon_pid" 2>/dev/null || true
  fi
  remove_runtime_dir
  exit "$status"
}

signal_exit() {
  exit "$1"
}

main() {
  local dry_run=false
  case "${1:-}" in
    "") ;;
    --dry-run) dry_run=true ;;
    -h|--help)
      usage
      return
      ;;
    *)
      usage >&2
      die "unknown argument '$1'"
      ;;
  esac
  [[ $# -le 1 ]] || die "expected at most one argument"
  [[ -n "${HOME:-}" ]] || die "HOME is not set"

  require_command cargo
  require_command pnpm

  if [[ "$dry_run" == false || -z "${SENTINAL_TAILSCALE_IP:-}" ]]; then
    require_tailscale_running
  fi

  local tailscale_ip daemon_port web_port data_dir config_dir token_file auth_file vite_bin
  tailscale_ip="$(discover_tailscale_ipv4)"
  daemon_port="${SENTINAL_DAEMON_PORT:-7420}"
  web_port="${SENTINAL_WEB_PORT:-5173}"
  data_dir="${SENTINAL_DATA_DIR:-$HOME/.local/share/terminal-agent}"
  config_dir="${SENTINAL_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/terminal-agent}"
  token_file="$config_dir/token"
  auth_file="$config_dir/auth.json"
  vite_bin="$REPO_ROOT/web/node_modules/.bin/vite"

  validate_tailscale_ipv4 "$tailscale_ip"
  validate_port daemon "$daemon_port"
  validate_port web "$web_port"
  [[ "$daemon_port" != "$web_port" ]] || die "daemon and web ports must be different"
  [[ -x "$vite_bin" ]] || die "Vite is not installed; run 'pnpm -C web install' and try again"

  runtime_dir="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-network.XXXXXX")"
  local config_file="$runtime_dir/daemon.toml"
  {
    printf 'bind = %s\n' "$(toml_string "$tailscale_ip:$daemon_port")"
    printf 'data_dir = %s\n' "$(toml_string "$data_dir")"
    printf 'scrollback_lines = 10000\n'
    printf 'token_file = %s\n' "$(toml_string "$token_file")"
    printf 'auth_file = %s\n' "$(toml_string "$auth_file")"
  } >"$config_file"

  echo "Sentinal Tailscale development server"
  echo "Web UI:    http://$tailscale_ip:$web_port"
  echo "Daemon API: http://$tailscale_ip:$daemon_port"
  echo "Health:     http://$tailscale_ip:$daemon_port/api/v1/health"
  echo "Token file: $token_file"
  echo "First setup: cat $(printf '%q' "$token_file")"
  echo "Stop both services with Ctrl-C."

  if [[ "$dry_run" == true ]]; then
    echo
    echo "Generated daemon config:"
    cat "$config_file"
    echo
    echo "Commands:"
    echo "cargo build --locked -p daemon --bin sentinald"
    echo "TA_CONFIG=$(printf '%q' "$config_file") $REPO_ROOT/target/debug/sentinald"
    echo "cd $(printf '%q' "$REPO_ROOT/web") && VITE_DATA=api VITE_DAEMON_PROXY_TARGET=http://$tailscale_ip:$daemon_port $vite_bin --host $tailscale_ip --port $web_port --strictPort"
    return
  fi

  mkdir -p "$data_dir" "$config_dir"
  chmod 700 "$config_dir"

  echo
  echo "Building sentinald..."
  (cd "$REPO_ROOT" && cargo build --locked -p daemon --bin sentinald)

  local target_dir="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
  if [[ "$target_dir" != /* ]]; then
    target_dir="$REPO_ROOT/$target_dir"
  fi
  local daemon_bin="$target_dir/debug/sentinald"
  [[ -x "$daemon_bin" ]] || die "built daemon binary was not found at '$daemon_bin'"

  echo "Starting sentinald..."
  TA_CONFIG="$config_file" "$daemon_bin" &
  daemon_pid=$!

  echo "Starting Vite..."
  (
    cd "$REPO_ROOT/web"
    export VITE_DATA=api
    export VITE_DAEMON_PROXY_TARGET="http://$tailscale_ip:$daemon_port"
    exec "$vite_bin" --host "$tailscale_ip" --port "$web_port" --strictPort
  ) &
  web_pid=$!

  set +e
  while kill -0 "$daemon_pid" 2>/dev/null && kill -0 "$web_pid" 2>/dev/null; do
    sleep 0.1
  done
  local status=0
  if ! kill -0 "$daemon_pid" 2>/dev/null; then
    wait "$daemon_pid"
    status=$?
    echo "sentinal network: sentinald stopped with status $status" >&2
  else
    wait "$web_pid"
    status=$?
    echo "sentinal network: Vite stopped with status $status" >&2
  fi
  set -e
  return "$status"
}

trap cleanup EXIT
trap 'signal_exit 130' INT
trap 'signal_exit 143' TERM
trap 'signal_exit 129' HUP

main "$@"
