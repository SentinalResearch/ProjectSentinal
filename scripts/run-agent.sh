#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly SENTINAL_LAUNCH_ROLE="agent"
source "$SCRIPT_DIR/lib/launch-common.sh"

daemon_pid=""
runtime_dir=""

usage() {
  echo 'Usage: scripts/run-agent.sh [--dry-run]'
  echo "Starts only sentinald on this Machine's exact Tailscale IPv4 address."
}

cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$daemon_pid" ]]; then
    kill "$daemon_pid" 2>/dev/null || true
    wait "$daemon_pid" 2>/dev/null || true
  fi
  if [[ -n "$runtime_dir" && -d "$runtime_dir" ]]; then
    case "$runtime_dir" in
      "${TMPDIR:-/tmp}"/sentinal-agent.*) rm -rf -- "$runtime_dir" ;;
      *) echo "sentinal agent: refusing to remove unexpected runtime path '$runtime_dir'" >&2 ;;
    esac
  fi
  exit "$status"
}

signal_exit() { exit "$1"; }

main() {
  local dry_run=false
  case "${1:-}" in
    "") ;;
    --dry-run) dry_run=true ;;
    -h|--help) usage; return ;;
    *) usage >&2; sentinal_die "unknown argument '$1'" ;;
  esac
  [[ $# -le 1 ]] || sentinal_die "expected at most one argument"
  [[ -n "${HOME:-}" ]] || sentinal_die "HOME is not set"

  local tailscale_ip daemon_port data_dir config_dir token_file auth_file local_socket config_file
  tailscale_ip="$(sentinal_resolve_local_ip "$dry_run")"
  sentinal_require_command cargo
  daemon_port="${SENTINAL_DAEMON_PORT:-7420}"
  sentinal_validate_port daemon "$daemon_port"
  data_dir="${SENTINAL_DATA_DIR:-$HOME/.local/share/terminal-agent}"
  config_dir="${SENTINAL_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/terminal-agent}"
  token_file="$config_dir/token"
  auth_file="$config_dir/auth.json"
  local_socket="$data_dir/run/sentinald.sock"

  runtime_dir="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-agent.XXXXXX")"
  config_file="$runtime_dir/daemon.toml"
  sentinal_write_daemon_config "$config_file" "$tailscale_ip:$daemon_port" "$data_dir" "$token_file" "$auth_file" "$local_socket"

  echo "Sentinal worker Machine"
  echo "Daemon API:     http://$tailscale_ip:$daemon_port"
  echo "Health:         http://$tailscale_ip:$daemon_port/api/v1/health"
  echo "Local socket:   $local_socket"
  echo "Token file:     $token_file"
  echo "Token bootstrap: cat $(printf '%q' "$token_file")"
  echo "Local Watch:    SENTINAL_URL=ws://$tailscale_ip:$daemon_port/api/v1/ws sentinal watch"
  echo "Stop sentinald with Ctrl-C."

  if [[ "$dry_run" == true ]]; then
    echo
    echo "Generated daemon config:"
    cat "$config_file"
    echo
    echo "Commands:"
    echo "cargo build --locked -p daemon --bin sentinald"
    echo "TA_CONFIG=$(printf '%q' "$config_file") $REPO_ROOT/target/debug/sentinald"
    return
  fi

  sentinal_require_port_available "$tailscale_ip" "$daemon_port"
  mkdir -p "$data_dir" "$config_dir"
  chmod 700 "$config_dir" "$data_dir"
  (cd "$REPO_ROOT" && cargo build --locked -p daemon --bin sentinald)

  local target_dir="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
  [[ "$target_dir" == /* ]] || target_dir="$REPO_ROOT/$target_dir"
  local daemon_bin="$target_dir/debug/sentinald"
  [[ -x "$daemon_bin" ]] || sentinal_die "built daemon binary was not found at '$daemon_bin'"

  TA_CONFIG="$config_file" "$daemon_bin" &
  daemon_pid=$!
  set +e
  wait "$daemon_pid"
  local status=$?
  set -e
  daemon_pid=""
  return "$status"
}

trap cleanup EXIT
trap 'signal_exit 130' INT
trap 'signal_exit 143' TERM
trap 'signal_exit 129' HUP
main "$@"
