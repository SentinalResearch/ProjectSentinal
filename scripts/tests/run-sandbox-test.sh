#!/usr/bin/env bash
# The sandbox never shares data, config, or ports with an installed Sentinal.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
launcher="$repo_root/scripts/run-sandbox.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-sandbox-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "run-sandbox test failed: $*" >&2
  exit 1
}

# A home with an installed control, whose settings must not leak in.
home="$test_root/home"
mkdir -p "$home/.config/terminal-agent"
printf '[packages]\npublic_base_url = "https://installed.example"\n' >"$home/.config/terminal-agent/daemon.toml"

output="$(HOME="$home" SENTINAL_TAILSCALE_IP=100.64.12.34 "$launcher" --dry-run 2>&1)"
sandbox="$home/.local/share/sentinal-sandbox"
for expected in \
  'bind = "100.64.12.34:7520"' \
  "data_dir = \"$sandbox/data\"" \
  "token_file = \"$sandbox/config/token\"" \
  "local_socket = \"$sandbox/data/run/sentinald.sock\"" \
  "config_dir = \"$sandbox/config\"" \
  'http://100.64.12.34:5273' \
  'SANDBOX:'; do
  [[ "$output" == *"$expected"* ]] || fail "missing: $expected"
done
for forbidden in '.local/share/terminal-agent' '.config/terminal-agent' 'installed.example' ':7420'; do
  [[ "$output" != *"$forbidden"* ]] || fail "leaked: $forbidden"
done

# It reports the release this checkout is based on, as production does.
latest_tag="$(git -C "$repo_root" describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
if [[ -n "$latest_tag" ]]; then
  [[ "$output" == *"reports ${latest_tag#v}"* ]] || fail "does not report ${latest_tag#v}"
fi

# --reset erases the sandbox only, and refuses a path not named as one.
mkdir -p "$sandbox/data"
HOME="$home" "$launcher" --reset >/dev/null
[[ ! -e "$sandbox" ]] || fail "--reset left the sandbox"
[[ -d "$home/.config/terminal-agent" ]] || fail "--reset touched the installed config"

precious="$test_root/precious"
mkdir -p "$precious"
if SENTINAL_SANDBOX_DIR="$precious" "$launcher" --reset >/dev/null 2>&1; then
  fail "--reset accepted a path not named sandbox"
fi
[[ -d "$precious" ]] || fail "--reset erased a path not named sandbox"

# A fresh sandbox finishes first-time setup itself, with a random password that
# is saved privately and never appears on a command line.
fixture="$test_root/fixture"
mkdir -p "$fixture/scripts" "$fixture/bin" "$fixture/home"
cp "$launcher" "$fixture/scripts/run-sandbox.sh"
sandbox="$fixture/home/.local/share/sentinal-sandbox"
cat >"$fixture/scripts/run-network.sh" <<'EOF_NETWORK'
#!/usr/bin/env bash
# Stands in for the real launcher: the daemon has written its token.
mkdir -p "$SENTINAL_CONFIG_DIR"
echo fake-token >"$SENTINAL_CONFIG_DIR/token"
for _ in $(seq 1 100); do
  [[ -s "$SENTINAL_CONFIG_DIR/sandbox-password" ]] && exit 0
  perl -e 'select(undef, undef, undef, 0.05)'
done
exit 1
EOF_NETWORK
cat >"$fixture/bin/curl" <<'EOF_CURL'
#!/usr/bin/env bash
log="$(dirname "$0")/curl.log"
echo "ARGV $*" >>"$log"
case "$*" in
  *auth/status*) printf '{"setup_required":%s}' "$(cat "$(dirname "$0")/setup-required")" ;;
  *auth/setup*) cat >>"$log"; echo false >"$(dirname "$0")/setup-required" ;;
  *) exit 22 ;;
esac
EOF_CURL
echo true >"$fixture/bin/setup-required"
chmod 700 "$fixture/scripts/run-sandbox.sh" "$fixture/scripts/run-network.sh" "$fixture/bin/curl"

run_fixture() {
  HOME="$fixture/home" PATH="$fixture/bin:$PATH" SENTINAL_TAILSCALE_IP=100.64.12.34 \
    "$fixture/scripts/run-sandbox.sh" 2>&1
}
first="$(run_fixture)" || fail "the sandbox launcher failed: $first"
for _ in $(seq 1 40); do
  [[ -s "$sandbox/config/sandbox-password" ]] && break
  perl -e 'select(undef, undef, undef, 0.05)'
done
saved="$sandbox/config/sandbox-password"
[[ -s "$saved" ]] || fail "no password was saved"
password="$(cat "$saved")"
[[ "$password" =~ ^[a-z2-9]{4}(-[a-z2-9]{4}){3}$ ]] || fail "unexpected password shape"
[[ "$(stat -f %Lp "$saved" 2>/dev/null || stat -c %a "$saved")" == 600 ]] || fail "the password file is not private"
log="$(cat "$fixture/bin/curl.log")"
[[ "$log" == *"Bearer fake-token"* && "$log" == *"\"password\\\":\\\"$password"* ]] || fail "setup did not send the token and password"
grep '^ARGV' "$fixture/bin/curl.log" | grep -Eq "fake-token|$password" && fail "a secret reached a command line"

# The next start keeps the password rather than replacing it.
second="$(run_fixture)" || fail "the second start failed: $second"
perl -e 'select(undef, undef, undef, 0.3)'
[[ "$(cat "$saved")" == "$password" ]] || fail "a later start changed the password"

echo "run-sandbox test: passed"
