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

assert_not_contains() {
  local haystack="$1"
  local needle="$2"
  [[ "$haystack" != *"$needle"* ]] || fail "expected output not to contain: $needle"
}

fake_bin="$test_root/fake-bin"
mkdir -p "$fake_bin"
cat >"$fake_bin/tailscale" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  status) echo 'Tailscale is stopped.'; exit 1 ;;
  ip) echo '100.93.101.56' ;;
  *) exit 2 ;;
esac
EOF
chmod 700 "$fake_bin/tailscale"

if stopped_output="$(PATH="$fake_bin:$PATH" "$launcher" --dry-run 2>&1)"; then
  fail "a stopped Tailscale backend was accepted"
fi
assert_contains "$stopped_output" "Tailscale is not running"
assert_contains "$stopped_output" "Turn Tailscale on"

assert_not_contains_packages() {
  [[ "$1" != *"[packages]"* ]] || fail "dev config gained a [packages] section with no installed one"
}
no_packages="$(SENTINAL_TAILSCALE_IP=100.64.12.34 SENTINAL_DATA_DIR="$test_root/d0" \
  SENTINAL_CONFIG_DIR="$test_root/empty-config" "$launcher" --dry-run 2>&1)"
assert_not_contains_packages "$no_packages"

# An installed control config that serves worker packages keeps doing so
# while `just network` runs (it uses the same port and directories).
mkdir -p "$test_root/config with spaces"
cat > "$test_root/config with spaces/daemon.toml" <<'TOML'
bind = "100.64.12.34:7420"

[packages]
enabled = true
listen = "127.0.0.1:7422"
directory = "/srv/sentinal/packages"
public_base_url = "https://control.example.ts.net"

[other]
ignored = true
TOML

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
assert_contains "$dry_run" 'development = true'
assert_contains "$dry_run" 'allowed_origins = ["http://100.64.12.34:14173"]'
assert_contains "$dry_run" '[control]'
assert_contains "$dry_run" '[packages]'
assert_contains "$dry_run" 'directory = "/srv/sentinal/packages"'
assert_contains "$dry_run" 'public_base_url = "https://control.example.ts.net"'
[[ "$dry_run" != *"[other]"* ]] || fail "copied more than the [packages] section"
assert_contains "$dry_run" 'public_url = "http://100.64.12.34:17420"'
assert_contains "$dry_run" "cd $repo_root/web &&"
assert_contains "$dry_run" '/web/node_modules/.bin/vite --host 100.64.12.34 --port 14173 --strictPort'
assert_not_contains "$dry_run" 'pnpm -C web dev'

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
