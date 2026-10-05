#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly SENTINAL_LAUNCH_ROLE="control"
source "$SCRIPT_DIR/lib/launch-common.sh"

usage() {
  echo 'Usage: scripts/run-control.sh [WORKER_IP=100.x.y.z] [--dry-run]'
  echo 'Starts only the real-data Vite UI.'
  echo 'Without WORKER_IP, it uses the installed persistent control daemon.'
}

config_has_control_role() {
  local file="$1"
  awk '
    /^[[:space:]]*\[/ { in_control = ($0 ~ /^[[:space:]]*\[control\][[:space:]]*$/) }
    in_control && /^[[:space:]]*enabled[[:space:]]*=[[:space:]]*true[[:space:]]*$/ { found = 1 }
    END { exit !found }
  ' "$file"
}

read_config_bind() {
  local file="$1"
  sed -n 's/^[[:space:]]*bind[[:space:]]*=[[:space:]]*"\([^"]*\)"[[:space:]]*$/\1/p' "$file" | head -1
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
  local tailscale_ip daemon_port web_port vite_bin target target_label
  local config_dir config_file use_persistent=false
  tailscale_ip="$(sentinal_resolve_local_ip "$dry_run")"
  daemon_port="${SENTINAL_DAEMON_PORT:-7420}"
  web_port="${SENTINAL_WEB_PORT:-5173}"
  sentinal_validate_port web "$web_port"
  vite_bin="$(sentinal_resolve_vite_bin "$REPO_ROOT")"
  config_dir="${SENTINAL_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/terminal-agent}"
  config_file="$config_dir/daemon.toml"

  if [[ -z "$worker_ip" ]]; then
    [[ -f "$config_file" ]] || sentinal_die "persistent daemon config was not found at '$config_file'; pass WORKER_IP=100.x.y.z for direct-worker mode"
    config_has_control_role "$config_file" || sentinal_die "persistent daemon config '$config_file' does not enable [control]"
    use_persistent=true
  else
    sentinal_validate_tailscale_ipv4 "$worker_ip"
    if [[ "$worker_ip" == "$tailscale_ip" && -f "$config_file" ]] && config_has_control_role "$config_file"; then
      use_persistent=true
    fi
  fi

  if [[ "$use_persistent" == false ]]; then
    sentinal_validate_port daemon "$daemon_port"
    target="http://$worker_ip:$daemon_port"
    target_label="Selected worker: $target"
  else
    local bind host port
    bind="$(read_config_bind "$config_file")"
    [[ -n "$bind" && "$bind" == *:* ]] || sentinal_die "persistent daemon config '$config_file' has no supported IPv4 bind"
    host="${bind%:*}"
    port="${bind##*:}"
    sentinal_validate_port daemon "$port"
    if [[ "$host" != "127.0.0.1" ]]; then
      sentinal_validate_tailscale_ipv4 "$host"
    fi
    target="http://$host:$port"
    target_label="Persistent control: $target"
  fi

  echo "Sentinal control UI"
  echo "Control UI: http://$tailscale_ip:$web_port"
  echo "$target_label"
  echo "API health:  $target/api/v1/health"
  echo "Stop Vite with Ctrl-C."

  if [[ "$dry_run" == true ]]; then
    echo
    echo "Command:"
    echo "cd $(printf '%q' "$REPO_ROOT/web") && VITE_DATA=api VITE_DAEMON_PROXY_TARGET=$target $(printf '%q' "$vite_bin") --host $tailscale_ip --port $web_port --strictPort"
    return
  fi

  sentinal_require_port_available "$tailscale_ip" "$web_port"
  cd "$REPO_ROOT/web"
  export VITE_DATA=api
  export VITE_DAEMON_PROXY_TARGET="$target"
  exec "$vite_bin" --host "$tailscale_ip" --port "$web_port" --strictPort
}

main "$@"
