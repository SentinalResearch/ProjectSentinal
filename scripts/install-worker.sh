#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly SENTINAL_LAUNCH_ROLE="installer"
source "$SCRIPT_DIR/lib/launch-common.sh"
source "$SCRIPT_DIR/lib/install-common.sh"

usage() {
  cat <<'EOF'
Usage:
  scripts/install-worker.sh install [OPTIONS]
  scripts/install-worker.sh uninstall [OPTIONS]

Install options:
  --source-dir DIR       build from a checked-out source tree (default: repository root)
  --release-dir DIR      install a prebuilt bin/ + shell/ release directory
  --prefix DIR           program prefix (default: ~/.local)
  --config-dir DIR       config and credentials (default: ~/.config/terminal-agent)
  --data-dir DIR         SQLite, logs, audit, adapters, recipes (default: ~/.local/share/terminal-agent)
  --bind IP              exact Tailscale IPv4 (default: discover this Machine)
  --port PORT            daemon port (default: 7420)
  --no-service           do not render, enable, start, stop, or remove a user service
  --dry-run              print actions without building or writing

Uninstall additionally accepts:
  --delete-data PATH     delete data only when PATH exactly resolves to --data-dir
EOF
}

command_name="${1:-}"
[[ -n "$command_name" ]] || { usage >&2; exit 2; }
shift

source_dir="$REPO_ROOT"
source_explicit=false
release_dir=""
release_explicit=false
prefix="${HOME:-}/.local"
prefix_explicit=false
config_dir="${XDG_CONFIG_HOME:-${HOME:-}/.config}/terminal-agent"
data_dir="${HOME:-}/.local/share/terminal-agent"
bind_ip=""
daemon_port=7420
install_service=true
dry_run=false
delete_data_confirmation=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --source-dir) source_dir="${2:-}"; source_explicit=true; shift 2 ;;
    --release-dir) release_dir="${2:-}"; release_explicit=true; shift 2 ;;
    --prefix) prefix="${2:-}"; prefix_explicit=true; shift 2 ;;
    --config-dir) config_dir="${2:-}"; shift 2 ;;
    --data-dir) data_dir="${2:-}"; shift 2 ;;
    --bind) bind_ip="${2:-}"; shift 2 ;;
    --port) daemon_port="${2:-}"; shift 2 ;;
    --delete-data) delete_data_confirmation="${2:-}"; shift 2 ;;
    --no-service) install_service=false; shift ;;
    --dry-run) dry_run=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) sentinal_install_die "unknown argument '$1'" ;;
  esac
done

[[ -n "${HOME:-}" ]] || sentinal_install_die "HOME is not set"
case "$command_name" in install|uninstall) ;; *) sentinal_install_die "unknown command '$command_name'" ;; esac
[[ "$source_explicit" != true || "$release_explicit" != true ]] || sentinal_install_die "use only one of --source-dir and --release-dir"
sentinal_require_safe_path prefix "$prefix"
sentinal_require_safe_path config "$config_dir"
sentinal_require_safe_path data "$data_dir"
sentinal_validate_port daemon "$daemon_port"
platform="$(sentinal_install_platform)"

bin_dir="$prefix/bin"
share_dir="$prefix/share/sentinal"
config_file="$config_dir/daemon.toml"
if [[ "$prefix_explicit" == true ]]; then
  service_dir="$prefix/service"
elif [[ "$platform" == macos ]]; then
  service_dir="$HOME/Library/LaunchAgents"
else
  service_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
fi
if [[ "$platform" == macos ]]; then
  service_file="$service_dir/dev.sentinal.sentinald.plist"
else
  service_file="$service_dir/sentinald.service"
fi

if [[ "$command_name" == uninstall ]]; then
  if [[ -n "$delete_data_confirmation" ]]; then
    sentinal_require_safe_path deletion "$delete_data_confirmation"
    expected_data="$(sentinal_canonical_existing_path "$data_dir")"
    confirmed_data="$(sentinal_canonical_existing_path "$delete_data_confirmation")"
    [[ "$confirmed_data" == "$expected_data" ]] || sentinal_install_die "data deletion confirmation must exactly equal '$data_dir'"
  fi
  if [[ "$dry_run" == true ]]; then
    echo "DRY RUN — would remove $bin_dir/sentinal, $bin_dir/sentinald, and $share_dir"
    [[ "$install_service" == false ]] || echo "DRY RUN — would disable and remove service: $service_file"
    [[ -z "$delete_data_confirmation" ]] || echo "DRY RUN — would delete data: $data_dir"
    exit 0
  fi
  [[ "$install_service" == false ]] || sentinal_disable_service "$platform"
  rm -f -- "$bin_dir/sentinal" "$bin_dir/sentinald"
  if [[ -d "$share_dir" ]]; then
    sentinal_require_safe_path share "$share_dir"
    rm -rf -- "$share_dir"
  fi
  [[ "$install_service" == false ]] || rm -f -- "$service_file"
  [[ "$install_service" == false ]] || sentinal_reload_service_manager "$platform"
  echo "Removed Sentinal worker programs from $prefix"
  echo "Preserved config: $config_dir"
  if [[ -n "$delete_data_confirmation" ]]; then
    sentinal_require_safe_path data "$data_dir"
    rm -rf -- "$data_dir"
    echo "Deleted data: $data_dir"
  else
    echo "Preserved data: $data_dir"
  fi
  exit 0
fi

[[ -z "$delete_data_confirmation" ]] || sentinal_install_die "--delete-data is valid only with uninstall"
if [[ -z "$bind_ip" ]]; then
  bind_ip="$(sentinal_resolve_local_ip "$dry_run")"
else
  sentinal_validate_tailscale_ipv4 "$bind_ip"
fi

if [[ -n "$release_dir" ]]; then
  sentinal_require_safe_path release "$release_dir"
  source_bin_dir="$release_dir/bin"
  shell_source="$release_dir/shell"
  build_command=""
else
  sentinal_require_safe_path source "$source_dir"
  source_bin_dir="$source_dir/target/release"
  shell_source="$source_dir/shell"
  build_command="cargo build --locked --offline --release -p daemon --bin sentinald -p cli --bin sentinal"
fi

[[ -d "$shell_source" ]] || sentinal_install_die "missing shell assets at '$shell_source'"
for asset in ta.zsh ta.bash ta.fish external/sentinal.zsh external/sentinal.bash external/sentinal.fish vendor/bash-preexec.sh; do
  [[ -f "$shell_source/$asset" && ! -L "$shell_source/$asset" ]] || sentinal_install_die "missing shell asset '$shell_source/$asset'"
done

if [[ -e "$config_file" || -L "$config_file" ]]; then
  [[ -f "$config_file" && ! -L "$config_file" ]] || sentinal_install_die "existing daemon config '$config_file' is not a regular file"
fi

if [[ "$dry_run" == true ]]; then
  echo "DRY RUN — Sentinal worker installation"
  [[ -z "$build_command" ]] || echo "cd $(printf '%q' "$source_dir") && $build_command"
  echo "Would install: $bin_dir/sentinal"
  echo "Would install: $bin_dir/sentinald"
  echo "Would install shell assets: $share_dir/shell"
  echo "Would create config if absent: $config_file"
  echo "Worker bind: $bind_ip:$daemon_port"
  if [[ "$install_service" == true ]]; then
    echo "Would install and enable service: $service_file"
  else
    echo "Service installation: disabled"
  fi
  exit 0
fi

if [[ -n "$build_command" ]]; then
  sentinal_require_command cargo
  (cd "$source_dir" && cargo build --locked --offline --release -p daemon --bin sentinald -p cli --bin sentinal)
fi
sentinal_validate_source_binary "$source_bin_dir/sentinal"
sentinal_validate_source_binary "$source_bin_dir/sentinald"

mkdir -p "$bin_dir" "$share_dir" "$config_dir" "$data_dir"
chmod 700 "$config_dir" "$data_dir"
sentinal_atomic_install "$source_bin_dir/sentinal" "$bin_dir/sentinal"
sentinal_atomic_install "$source_bin_dir/sentinald" "$bin_dir/sentinald"

shell_destination="$share_dir/shell"
shell_temporary="$share_dir/shell.tmp.$$"
rm -rf -- "$shell_temporary"
mkdir -p "$shell_temporary"
cp -R "$shell_source/." "$shell_temporary/"
rm -rf -- "$shell_destination"
mv "$shell_temporary" "$shell_destination"

if [[ -e "$config_file" ]]; then
  echo "Preserved existing daemon config: $config_file"
else
  temporary_config="$config_file.tmp.$$"
  sentinal_write_daemon_config "$temporary_config" "$bind_ip:$daemon_port" "$data_dir" "$config_dir/token" "$config_dir/auth.json" "$data_dir/run/sentinald.sock"
  chmod 600 "$temporary_config"
  mv "$temporary_config" "$config_file"
fi

if [[ "$install_service" == true ]]; then
  mkdir -p "$service_dir"
  temporary_service="$service_file.tmp.$$"
  if [[ "$platform" == macos ]]; then
    sentinal_render_launchd "$bin_dir/sentinald" "$config_file" "$data_dir/sentinald.stdout.log" "$data_dir/sentinald.stderr.log" >"$temporary_service"
  else
    sentinal_render_systemd "$bin_dir/sentinald" "$config_file" >"$temporary_service"
  fi
  chmod 600 "$temporary_service"
  mv "$temporary_service" "$service_file"
  sentinal_enable_service "$platform" "$service_file"
fi

echo "Installed sentinal and sentinald in $bin_dir"
echo "Installed shell assets in $shell_destination"
echo "Worker bind: $bind_ip:$daemon_port"
echo "Config: $config_file"
echo "Data retained at: $data_dir"
if [[ "$install_service" == false ]]; then
  echo "Service installation: disabled"
else
  echo "Service: $service_file"
fi
echo "FULL Sessions are interrupted when sentinald restarts until Reliability R1 lands."
