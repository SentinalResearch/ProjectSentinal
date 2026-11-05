#!/usr/bin/env bash
# `just dev-cli ARGS`: run this checkout's `sentinal` against the dev environment.
set -euo pipefail
readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="${SENTINAL_DEV_DIR:-$HOME/.local/share/sentinal-dev}"
[[ -s "$root/config/token" ]] || { echo "sentinal dev: not started yet; run: just dev" >&2; exit 1; }
ip="${SENTINAL_TAILSCALE_IP:-$(tailscale ip -4 | head -1)}"
# `just dev` built it, stamped with the version; never replace it with an unstamped one.
bin="${CARGO_TARGET_DIR:-$REPO_ROOT/target}/debug/sentinal"
[[ -x "$bin" ]] || { echo "sentinal dev: not built yet; run: just dev" >&2; exit 1; }
export SENTINAL_ISOLATED=1
export SENTINAL_URL="ws://$ip:${SENTINAL_DEV_DAEMON_PORT:-7520}/api/v1/ws"
export SENTINAL_TOKEN_FILE="$root/config/token"
export SENTINAL_SOCKET="$root/data/run/sentinald.sock"
unset SENTINAL_SESSION_ID
exec "$bin" "$@"
