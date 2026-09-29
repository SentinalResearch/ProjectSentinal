#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
network_launcher="$repo_root/scripts/run-network.sh"
agent_launcher="$repo_root/scripts/run-agent.sh"
control_launcher="$repo_root/scripts/run-control.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-role-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "run-role test failed: $*" >&2
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

run_dry() {
  local launcher="$1"
  shift
  SENTINAL_TAILSCALE_IP=100.64.12.34 \
  SENTINAL_DAEMON_PORT=17420 \
  SENTINAL_WEB_PORT=14173 \
  SENTINAL_DATA_DIR="$test_root/data with spaces" \
  SENTINAL_CONFIG_DIR="$test_root/config with spaces" \
  VITE_DATA=mock \
    "$launcher" "$@" --dry-run 2>&1
}

network_output="$(run_dry "$network_launcher")"
assert_contains "$network_output" 'bind = "100.64.12.34:17420"'
assert_contains "$network_output" 'VITE_DATA=api'
assert_contains "$network_output" 'VITE_DAEMON_PROXY_TARGET=http://100.64.12.34:17420'
assert_contains "$network_output" "data_dir = \"$test_root/data with spaces\""

agent_output="$(run_dry "$agent_launcher")"
assert_contains "$agent_output" 'Sentinal worker Machine'
assert_contains "$agent_output" 'bind = "100.64.12.34:17420"'
assert_contains "$agent_output" "Token bootstrap: cat $test_root/config\\ with\\ spaces/token"
assert_contains "$agent_output" "$test_root/data with spaces/run/sentinald.sock"
assert_contains "$agent_output" 'cargo build --locked -p daemon --bin sentinald'
assert_not_contains "$agent_output" 'VITE_DATA='
assert_not_contains "$agent_output" 'Starting Vite'

control_output="$(run_dry "$control_launcher" WORKER_IP=100.90.80.70)"
assert_contains "$control_output" 'Sentinal control UI'
assert_contains "$control_output" 'Control UI: http://100.64.12.34:14173'
assert_contains "$control_output" 'Selected worker: http://100.90.80.70:17420'
assert_contains "$control_output" 'VITE_DATA=api'
assert_contains "$control_output" 'VITE_DAEMON_PROXY_TARGET=http://100.90.80.70:17420'
assert_contains "$control_output" '--host 100.64.12.34 --port 14173 --strictPort'
assert_not_contains "$control_output" 'cargo build'
assert_not_contains "$control_output" 'sentinald'

for launcher_and_args in \
  "$network_launcher" \
  "$agent_launcher" \
  "$control_launcher WORKER_IP=100.90.80.70"; do
  read -r launcher first_arg <<<"$launcher_and_args"
  for unsafe_ip in 0.0.0.0 :: 192.168.1.10 100.128.0.1; do
    if output="$(SENTINAL_TAILSCALE_IP="$unsafe_ip" "$launcher" ${first_arg:+"$first_arg"} --dry-run 2>&1)"; then
      fail "unsafe local address $unsafe_ip was accepted by $launcher"
    fi
    assert_contains "$output" 'is not a Tailscale IPv4 address'
  done
done

for unsafe_worker in 0.0.0.0 :: 10.0.0.7 100.63.255.255; do
  if output="$(SENTINAL_TAILSCALE_IP=100.64.0.1 "$control_launcher" "WORKER_IP=$unsafe_worker" --dry-run 2>&1)"; then
    fail "unsafe worker address $unsafe_worker was accepted"
  fi
  assert_contains "$output" 'is not a Tailscale IPv4 address'
done

if output="$(SENTINAL_TAILSCALE_IP=100.64.0.1 "$control_launcher" --dry-run 2>&1)"; then
  fail 'control accepted a missing WORKER_IP'
fi
assert_contains "$output" 'WORKER_IP=100.x.y.z is required'

for bad_port in 0 65536 abc; do
  if output="$(SENTINAL_TAILSCALE_IP=100.64.0.1 SENTINAL_DAEMON_PORT="$bad_port" "$agent_launcher" --dry-run 2>&1)"; then
    fail "invalid daemon port $bad_port was accepted"
  fi
  assert_contains "$output" "invalid daemon port '$bad_port'"

  if output="$(SENTINAL_TAILSCALE_IP=100.64.0.1 SENTINAL_WEB_PORT="$bad_port" "$control_launcher" WORKER_IP=100.80.0.1 --dry-run 2>&1)"; then
    fail "invalid web port $bad_port was accepted"
  fi
  assert_contains "$output" "invalid web port '$bad_port'"
done

fake_bin="$test_root/fake-bin"
mkdir -p "$fake_bin"

cat >"$fake_bin/tailscale" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  status) echo '100.64.12.34 test-machine user@ macOS -';;
  ip) echo '100.64.12.34';;
  *) exit 2;;
esac
EOF

cat >"$fake_bin/lsof" <<'EOF'
#!/usr/bin/env bash
if [[ "$*" == *":${OCCUPIED_PORT:-never}"* ]]; then
  echo 'occupied'
  exit 0
fi
exit 1
EOF

cat >"$fake_bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p "$CARGO_TARGET_DIR/debug"
cat >"$CARGO_TARGET_DIR/debug/sentinald" <<'DAEMON'
#!/usr/bin/env bash
trap 'echo daemon-stopped >>"$ROLE_TEST_EVENTS"; exit 0' TERM INT HUP
echo daemon-started >>"$ROLE_TEST_EVENTS"
while :; do sleep 0.02; done
DAEMON
chmod 700 "$CARGO_TARGET_DIR/debug/sentinald"
EOF

cat >"$fake_bin/vite" <<'EOF'
#!/usr/bin/env bash
trap 'echo vite-stopped >>"$ROLE_TEST_EVENTS"; exit 0' TERM INT HUP
echo "vite-started VITE_DATA=$VITE_DATA TARGET=$VITE_DAEMON_PROXY_TARGET" >>"$ROLE_TEST_EVENTS"
while :; do sleep 0.02; done
EOF

chmod 700 "$fake_bin/tailscale" "$fake_bin/lsof" "$fake_bin/cargo" "$fake_bin/vite"

if output="$(
  PATH="$fake_bin:/usr/bin:/bin" \
  OCCUPIED_PORT=17420 \
  SENTINAL_DAEMON_PORT=17420 \
  SENTINAL_DATA_DIR="$test_root/data" \
  SENTINAL_CONFIG_DIR="$test_root/config" \
    "$agent_launcher" 2>&1
)"; then
  fail 'agent accepted an occupied daemon port'
fi
assert_contains "$output" "100.64.12.34:17420 is already in use"

if output="$(
  PATH="$fake_bin:/usr/bin:/bin" \
  OCCUPIED_PORT=14173 \
  SENTINAL_WEB_PORT=14173 \
  SENTINAL_VITE_BIN="$fake_bin/vite" \
    "$control_launcher" WORKER_IP=100.90.80.70 2>&1
)"; then
  fail 'control accepted an occupied web port'
fi
assert_contains "$output" "100.64.12.34:14173 is already in use"

if output="$(PATH=/usr/bin:/bin SENTINAL_TAILSCALE_IP= "$agent_launcher" --dry-run 2>&1)"; then
  fail 'agent accepted a missing Tailscale command'
fi
assert_contains "$output" "required command 'tailscale' was not found"

events="$test_root/events"
: >"$events"
PATH="$fake_bin:/usr/bin:/bin" \
ROLE_TEST_EVENTS="$events" \
CARGO_TARGET_DIR="$test_root/target" \
SENTINAL_VITE_BIN="$fake_bin/vite" \
SENTINAL_DATA_DIR="$test_root/live data" \
SENTINAL_CONFIG_DIR="$test_root/live config" \
  "$network_launcher" >"$test_root/network.out" 2>&1 &
launcher_pid=$!

for _ in {1..100}; do
  if grep -q daemon-started "$events" && grep -q vite-started "$events"; then
    break
  fi
  sleep 0.01
done
grep -q daemon-started "$events" || fail 'combined launcher did not start daemon child'
grep -q 'vite-started VITE_DATA=api TARGET=http://100.64.12.34:7420' "$events" || fail 'combined launcher did not start API-mode Vite child'

kill -TERM "$launcher_pid"
wait "$launcher_pid" || status=$?
[[ "${status:-0}" == 143 ]] || fail "combined launcher exited with ${status:-0}, expected 143"
grep -q daemon-stopped "$events" || fail 'combined launcher left daemon child running'
grep -q vite-stopped "$events" || fail 'combined launcher left Vite child running'

echo 'run-role test: passed'
