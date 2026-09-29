#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly SENTINAL_LAUNCH_ROLE="control"
source "$SCRIPT_DIR/lib/launch-common.sh"

usage() {
  echo 'Usage: scripts/run-control.sh WORKER_IP=100.x.y.z [--dry-run]'
  echo 'Starts only the real-data Vite UI for one selected worker Machine.'
}

main() {
  local dry_run=false worker_ip="" argument
  for argument in "$@"; do
    case "$argument" in
      --dry-run) dry_run=true ;;
      WORKER_IP=*) worker_ip="${argument#WORKER_IP=}" ;;
      -h|--help) usage; return ;;
      *) usage >&2; sentinal_die "unknown argument '$argument'" ;;
    esac
  done
  [[ -n "$worker_ip" ]] || sentinal_die "WORKER_IP=100.x.y.z is required"

  local tailscale_ip daemon_port web_port vite_bin
  tailscale_ip="$(sentinal_resolve_local_ip "$dry_run")"
  sentinal_validate_tailscale_ipv4 "$worker_ip"
  daemon_port="${SENTINAL_DAEMON_PORT:-7420}"
  web_port="${SENTINAL_WEB_PORT:-5173}"
  sentinal_validate_port daemon "$daemon_port"
  sentinal_validate_port web "$web_port"
  vite_bin="$(sentinal_resolve_vite_bin "$REPO_ROOT")"

  echo "Sentinal control UI"
  echo "Control UI: http://$tailscale_ip:$web_port"
  echo "Selected worker: http://$worker_ip:$daemon_port"
  echo "Worker health:   http://$worker_ip:$daemon_port/api/v1/health"
  echo "Stop Vite with Ctrl-C."

  if [[ "$dry_run" == true ]]; then
    echo
    echo "Command:"
    echo "cd $(printf '%q' "$REPO_ROOT/web") && VITE_DATA=api VITE_DAEMON_PROXY_TARGET=http://$worker_ip:$daemon_port $(printf '%q' "$vite_bin") --host $tailscale_ip --port $web_port --strictPort"
    return
  fi

  sentinal_require_port_available "$tailscale_ip" "$web_port"
  cd "$REPO_ROOT/web"
  export VITE_DATA=api
  export VITE_DAEMON_PROXY_TARGET="http://$worker_ip:$daemon_port"
  exec "$vite_bin" --host "$tailscale_ip" --port "$web_port" --strictPort
}

main "$@"
