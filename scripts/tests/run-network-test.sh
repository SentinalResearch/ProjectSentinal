#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
launcher="$repo_root/scripts/run-network.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-network-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "run-network test failed: $*" >&2
  exit 1
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  [[ "$haystack" == *"$needle"* ]] || fail "expected output to contain: $needle"
}

dry_run="$({
  SENTINAL_TAILSCALE_IP=100.64.12.34 \
  SENTINAL_DAEMON_PORT=17420 \
  SENTINAL_WEB_PORT=14173 \
  SENTINAL_DATA_DIR="$test_root/data with spaces" \
  SENTINAL_CONFIG_DIR="$test_root/config with spaces" \
  VITE_DATA=mock \
    "$launcher" --dry-run
} 2>&1)"

assert_contains "$dry_run" 'bind = "100.64.12.34:17420"'
assert_contains "$dry_run" "data_dir = \"$test_root/data with spaces\""
assert_contains "$dry_run" "Web UI:    http://100.64.12.34:14173"
assert_contains "$dry_run" "Daemon API: http://100.64.12.34:17420"
assert_contains "$dry_run" 'VITE_DAEMON_PROXY_TARGET=http://100.64.12.34:17420'
assert_contains "$dry_run" 'VITE_DATA=api'
assert_contains "$dry_run" 'pnpm -C web dev --host 100.64.12.34 --port 14173 --strictPort'

user_test="$(cd "$repo_root" && just --dry-run user-test 2>&1)"
assert_contains "$user_test" 'VITE_DATA=mock'
assert_contains "$user_test" 'pnpm -C web dev --host 127.0.0.1 --port 5173 --strictPort'

for unsafe_ip in 0.0.0.0 192.168.1.10 100.128.0.1; do
  if output="$(SENTINAL_TAILSCALE_IP="$unsafe_ip" "$launcher" --dry-run 2>&1)"; then
    fail "unsafe address $unsafe_ip was accepted"
  fi
  assert_contains "$output" "is not a Tailscale IPv4 address"
done

for bad_port in 0 65536 abc; do
  if output="$(SENTINAL_TAILSCALE_IP=100.64.0.1 SENTINAL_WEB_PORT="$bad_port" "$launcher" --dry-run 2>&1)"; then
    fail "invalid port $bad_port was accepted"
  fi
  assert_contains "$output" "invalid web port '$bad_port'"
done

echo "run-network test: passed"
