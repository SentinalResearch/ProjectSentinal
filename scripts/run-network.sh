#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly SENTINAL_LAUNCH_ROLE="network"
source "$SCRIPT_DIR/lib/launch-common.sh"

daemon_pid=""
web_pid=""
runtime_dir=""

usage() {
  echo 'Usage: scripts/run-network.sh [--dry-run]'
  echo "Starts sentinald and the real-data Vite UI on this Machine's exact Tailscale IPv4 address."
}

cleanup() {
  local status=$?
  trap - EXIT
  [[ -z "$web_pid" ]] || kill "$web_pid" 2>/dev/null || true
  [[ -z "$daemon_pid" ]] || kill "$daemon_pid" 2>/dev/null || true
  [[ -z "$web_pid" ]] || wait "$web_pid" 2>/dev/null || true
  [[ -z "$daemon_pid" ]] || wait "$daemon_pid" 2>/dev/null || true
  if [[ -n "$runtime_dir" && -d "$runtime_dir" ]]; then
    case "$runtime_dir" in
      "${TMPDIR:-/tmp}"/sentinal-network.*) rm -rf -- "$runtime_dir" ;;
      *) echo "sentinal network: refusing to remove unexpected runtime path '$runtime_dir'" >&2 ;;
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

  local tailscale_ip daemon_port web_port data_dir config_dir token_file auth_file local_socket vite_bin config_file
  tailscale_ip="$(sentinal_resolve_local_ip "$dry_run")"
  sentinal_require_command cargo
  daemon_port="${SENTINAL_DAEMON_PORT:-7420}"
  web_port="${SENTINAL_WEB_PORT:-5173}"
  sentinal_validate_port daemon "$daemon_port"
  sentinal_validate_port web "$web_port"
  [[ "$daemon_port" != "$web_port" ]] || sentinal_die "daemon and web ports must be different"
  data_dir="${SENTINAL_DATA_DIR:-$HOME/.local/share/terminal-agent}"
  config_dir="${SENTINAL_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/terminal-agent}"
  token_file="$config_dir/token"
  auth_file="$config_dir/auth.json"
  local_socket="$data_dir/run/sentinald.sock"
  vite_bin="$(sentinal_resolve_vite_bin "$REPO_ROOT")"

  runtime_dir="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-network.XXXXXX")"
  config_file="$runtime_dir/daemon.toml"

  # An installed control with a Tailscale Serve URL ([packages].public_base_url)
  # keeps serving worker packages, and its control API must sit behind Serve:
  # Serve on macOS can only proxy to loopback, and production workers accept
  # only an https://….ts.net control.
  local installed_config="$config_dir/daemon.toml" serve_url=""
  local daemon_host="$tailscale_ip" packages_section=""
  if [[ -f "$installed_config" ]] && grep -Eq '^\[packages\][[:space:]]*$' "$installed_config"; then
    packages_section="$(awk '/^\[packages\][[:space:]]*$/ {copy = 1; print; next} /^\[/ {copy = 0} copy' "$installed_config")"
    serve_url="$(printf '%s\n' "$packages_section" | sed -n 's/^public_base_url[[:space:]]*=[[:space:]]*"\(https:\/\/[^"]*\)".*/\1/p' | head -1)"
  fi
  local origins="\"http://$tailscale_ip:$web_port\"" public_url="http://$tailscale_ip:$daemon_port" serve_lines=()
  if [[ -n "$serve_url" ]]; then
    daemon_host="127.0.0.1"
    origins="$origins, \"$serve_url\""
    public_url="$serve_url"
    serve_lines=("behind_tailscale_serve = true")
  fi
  # Development control: plain-HTTP browser origin for this UI only.
  sentinal_write_daemon_config "$config_file" "$daemon_host:$daemon_port" "$data_dir" "$token_file" "$auth_file" "$local_socket" \
    "development = true" \
    ${serve_lines[@]+"${serve_lines[@]}"} \
    "allowed_origins = [$origins]" \
    "[control]" \
    "enabled = true" \
    "public_url = \"$public_url\""
  if [[ -n "$packages_section" ]]; then
    printf '\n%s\n' "$packages_section" >>"$config_file"
  fi

  echo "Sentinal Tailscale development server"
  echo "Web UI:    http://$tailscale_ip:$web_port"
  echo "Daemon API: http://$daemon_host:$daemon_port"
  echo "Health:     http://$daemon_host:$daemon_port/api/v1/health"
  echo "Token file: $token_file"
  echo "First setup: cat $(printf '%q' "$token_file")"
  if [[ -n "$serve_url" ]]; then
    echo "Packages:   served from the installed config (worker updates keep working)"
    echo "Workers:    sentinal connect $serve_url   (code from Settings → Machines → Add Machine)"
    if ! { tailscale serve status 2>/dev/null | grep -Fq "/api proxy http://127.0.0.1:$daemon_port/api"; }; then
      echo "Publish the control API once (HTTPS for workers):"
      echo "  tailscale serve --bg --https=443 --set-path /api http://127.0.0.1:$daemon_port/api"
    fi
  else
    echo "Add a worker: Settings → Machines → Add Machine, then on the worker: sentinal connect http://$tailscale_ip:$daemon_port"
  fi
  echo "Stop both services with Ctrl-C."

  if [[ "$dry_run" == true ]]; then
    echo
    echo "Generated daemon config:"
    cat "$config_file"
    echo
    echo "Commands:"
    echo "cargo build --locked -p daemon --bin sentinald"
    echo "TA_CONFIG=$(printf '%q' "$config_file") $REPO_ROOT/target/debug/sentinald"
    echo "cd $(printf '%q' "$REPO_ROOT/web") && VITE_DATA=api VITE_DAEMON_PROXY_TARGET=http://$daemon_host:$daemon_port $(printf '%q' "$vite_bin") --host $tailscale_ip --port $web_port --strictPort"
    return
  fi

  sentinal_require_port_available "$daemon_host" "$daemon_port"
  sentinal_require_port_available "$tailscale_ip" "$web_port"
  mkdir -p "$data_dir" "$config_dir"
  chmod 700 "$config_dir" "$data_dir"
  (cd "$REPO_ROOT" && cargo build --locked -p daemon --bin sentinald)

  local target_dir="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
  [[ "$target_dir" == /* ]] || target_dir="$REPO_ROOT/$target_dir"
  local daemon_bin="$target_dir/debug/sentinald"
  [[ -x "$daemon_bin" ]] || sentinal_die "built daemon binary was not found at '$daemon_bin'"

  TA_CONFIG="$config_file" "$daemon_bin" &
  daemon_pid=$!
  (
    cd "$REPO_ROOT/web"
    export VITE_DATA=api
    export VITE_DAEMON_PROXY_TARGET="http://$daemon_host:$daemon_port"
    exec "$vite_bin" --host "$tailscale_ip" --port "$web_port" --strictPort
  ) &
  web_pid=$!

  set +e
  while kill -0 "$daemon_pid" 2>/dev/null && kill -0 "$web_pid" 2>/dev/null; do
    sleep 0.05
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
