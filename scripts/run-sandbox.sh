#!/usr/bin/env bash
# `just sandbox`: this checkout's daemon and web UI with their own data, config
# and ports, so testing a change cannot touch the installed Sentinal's Sessions.
# Sessions still run real commands as you on this Machine: this isolates
# Sentinal's data, not the operating system.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  echo 'Usage: scripts/run-sandbox.sh [--dry-run | --reset]'
  echo "Runs this checkout on this Machine's Tailscale address with separate data."
  echo '--reset erases the sandbox data and exits.'
}

[[ -n "${HOME:-}" ]] || { echo "sentinal sandbox: HOME is not set" >&2; exit 1; }
root="${SENTINAL_SANDBOX_DIR:-$HOME/.local/share/sentinal-sandbox}"

case "${1:-}" in
  -h|--help) usage; exit 0 ;;
  --reset)
    case "$(basename "$root")" in
      *sandbox*) ;;
      *) echo "sentinal sandbox: refusing to erase '$root': its name does not say sandbox" >&2; exit 1 ;;
    esac
    rm -rf -- "$root"
    echo "Erased the sandbox at $root"
    exit 0
    ;;
  ""|--dry-run) ;;
  *) usage >&2; exit 1 ;;
esac

export SENTINAL_DATA_DIR="$root/data"
export SENTINAL_CONFIG_DIR="$root/config"
export SENTINAL_DAEMON_PORT="${SENTINAL_SANDBOX_DAEMON_PORT:-7520}"
export SENTINAL_WEB_PORT="${SENTINAL_SANDBOX_WEB_PORT:-5273}"

# First-time setup is pointless friction on a throwaway control, so do it here:
# once the daemon answers, set a random password and print it. It is random
# because this UI is reachable from the Tailnet and opens real terminals.
finish_setup() {
  local base="http://${SENTINAL_TAILSCALE_IP:-$(tailscale ip -4 | head -1)}:$SENTINAL_DAEMON_PORT/api/v1/auth"
  local token_file="$SENTINAL_CONFIG_DIR/token" saved="$SENTINAL_CONFIG_DIR/sandbox-password"
  local status="" attempt
  for attempt in $(seq 1 600); do
    if [[ -s "$token_file" ]] && status="$(curl -fsS -m 2 "$base/status" 2>/dev/null)"; then
      break
    fi
    status=""
    perl -e 'select(undef, undef, undef, 0.5)'
  done
  if [[ -z "$status" ]]; then
    echo "SANDBOX: the daemon did not answer, so first-time setup was not done." >&2
    return
  fi
  if [[ "$status" == *'"setup_required":true'* ]]; then
    local password
    password="$(LC_ALL=C tr -dc 'a-z2-9' </dev/urandom | head -c 16 | sed -E 's/(....)/\1-/g; s/-$//' || true)"
    # Through stdin: neither secret appears in `ps`.
    if ! printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\ndata = "{\\"password\\":\\"%s\\"}"\n' \
      "$(cat "$token_file")" "$password" | curl -fsS -m 5 -o /dev/null -K - "$base/setup"; then
      echo "SANDBOX: first-time setup failed; use the setup page instead." >&2
      return
    fi
    (umask 077; printf '%s\n' "$password" >"$saved")
  fi
  if [[ -s "$saved" ]]; then
    echo
    echo "SANDBOX: sign in with this password: $(cat "$saved")"
    echo
  else
    echo "SANDBOX: sign in with the password you chose, or run 'just sandbox --reset' to get a new one."
  fi
}

# Report the release this checkout is based on, as production does. `git
# describe` needs the tags; without them the build says 0.0.1, as before.
release_tag="$(git -C "$SCRIPT_DIR/.." describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
if [[ -n "$release_tag" ]]; then
  export SENTINAL_RELEASE_VERSION="${release_tag#v}"
  SENTINAL_GIT_COMMIT="$(git -C "$SCRIPT_DIR/.." rev-parse HEAD)"
  # Uncommitted changes are not what that commit contains: say so.
  [[ -z "$(git -C "$SCRIPT_DIR/.." status --porcelain 2>/dev/null)" ]] || SENTINAL_GIT_COMMIT+="-dirty"
  export SENTINAL_GIT_COMMIT
fi

echo "SANDBOX: separate data in $root (--reset erases it). Your installed Sentinal is not touched."
echo "SANDBOX: reports ${SENTINAL_RELEASE_VERSION:-a development version}, plus any changes not released yet. It never updates itself."
if [[ "${1:-}" != "--dry-run" ]]; then
  finish_setup &
fi
exec "$SCRIPT_DIR/run-network.sh" "$@"
