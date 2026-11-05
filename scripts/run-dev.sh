#!/usr/bin/env bash
# `just dev`: this checkout as an isolated single-Machine Sentinal. Its own data,
# config and ports; the daemon refuses any path of the installed Sentinal, never
# links to a control, and never checks for updates. Terminals in it reach this
# daemon. They still run real commands as you: this isolates Sentinal, not the
# operating system.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  echo 'Usage: scripts/run-dev.sh [--dry-run | --reset]'
  echo "Runs this checkout on this Machine's Tailscale address with separate data."
  echo '--reset erases the dev data and exits.'
}

[[ -n "${HOME:-}" ]] || { echo "sentinal dev: HOME is not set" >&2; exit 1; }
root="${SENTINAL_DEV_DIR:-$HOME/.local/share/sentinal-dev}"

case "${1:-}" in
  -h|--help) usage; exit 0 ;;
  --reset)
    case "$(basename "$root")" in
      *dev*) ;;
      *) echo "sentinal dev: refusing to erase '$root': its name does not say dev" >&2; exit 1 ;;
    esac
    rm -rf -- "$root"
    echo "Erased the dev environment at $root"
    exit 0
    ;;
  ""|--dry-run) ;;
  *) usage >&2; exit 1 ;;
esac

export SENTINAL_DATA_DIR="$root/data"
export SENTINAL_CONFIG_DIR="$root/config"
export SENTINAL_DAEMON_PORT="${SENTINAL_DEV_DAEMON_PORT:-7520}"
export SENTINAL_WEB_PORT="${SENTINAL_DEV_WEB_PORT:-5273}"
export SENTINAL_ISOLATED=true
export VITE_SENTINAL_ENV=development

# First-time setup is pointless friction on a throwaway control, so do it here:
# once the daemon answers, set a random password and print it. It is random
# because this UI is reachable from the Tailnet and opens real terminals.
finish_setup() {
  local base="http://${SENTINAL_TAILSCALE_IP:-$(tailscale ip -4 | head -1)}:$SENTINAL_DAEMON_PORT/api/v1/auth"
  local token_file="$SENTINAL_CONFIG_DIR/token" saved="$SENTINAL_CONFIG_DIR/dev-password"
  local status="" attempt
  for attempt in $(seq 1 600); do
    if [[ -s "$token_file" ]] && status="$(curl -fsS -m 2 "$base/status" 2>/dev/null)"; then
      break
    fi
    status=""
    perl -e 'select(undef, undef, undef, 0.5)'
  done
  if [[ -z "$status" ]]; then
    echo "DEV: the daemon did not answer, so first-time setup was not done." >&2
    return
  fi
  if [[ "$status" == *'"setup_required":true'* ]]; then
    local password
    password="$(LC_ALL=C tr -dc 'a-z2-9' </dev/urandom | head -c 16 | sed -E 's/(....)/\1-/g; s/-$//' || true)"
    # Through stdin: neither secret appears in `ps`.
    if ! printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\ndata = "{\\"password\\":\\"%s\\"}"\n' \
      "$(cat "$token_file")" "$password" | curl -fsS -m 5 -o /dev/null -K - "$base/setup"; then
      echo "DEV: first-time setup failed; use the setup page instead." >&2
      return
    fi
    (umask 077; printf '%s\n' "$password" >"$saved")
  fi
  if [[ -s "$saved" ]]; then
    echo
    echo "DEV: sign in with this password: $(cat "$saved")"
    echo
  else
    echo "DEV: sign in with the password you chose, or run 'just dev --reset' to get a new one."
  fi
}

# Report the release this checkout is based on, as production does. `git
# describe` needs the tags; without them the build says 0.0.1, as before.
release_tag="$(git -C "$SCRIPT_DIR/.." describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
if [[ -n "$release_tag" ]]; then
  export SENTINAL_RELEASE_VERSION="${release_tag#v}"
  SENTINAL_GIT_COMMIT="$(git -C "$SCRIPT_DIR/.." rev-parse HEAD)"
  # Uncommitted changes are not what that commit contains: say so.
  [[ -z "$(git -C "$SCRIPT_DIR/.." status --porcelain --untracked-files=no 2>/dev/null)" ]] || SENTINAL_GIT_COMMIT+="-dirty"
  export SENTINAL_GIT_COMMIT
fi

echo "DEV: isolated in $root (--reset erases it). No link to any control; production is not touched."
echo "DEV: based on ${SENTINAL_RELEASE_VERSION:-an unreleased version}; it never updates itself."
if [[ "${1:-}" != "--dry-run" ]]; then
  # Dev terminals run this checkout's `sentinal`, built alongside the daemon.
  (cd "$SCRIPT_DIR/.." && cargo build --locked -q -p cli --bin sentinal)
  finish_setup &
fi
exec "$SCRIPT_DIR/run-network.sh" "$@"
