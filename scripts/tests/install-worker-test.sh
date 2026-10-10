#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
installer="$repo_root/scripts/install-worker.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-installer-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "worker installer test failed: $*" >&2
  exit 1
}

assert_contains() {
  local haystack="$1" needle="$2"
  [[ "$haystack" == *"$needle"* ]] || fail "expected output to contain: $needle"
}

assert_file_contains() {
  local file="$1" needle="$2"
  grep -Fq "$needle" "$file" || fail "expected $file to contain: $needle"
}

assert_absent() {
  [[ ! -e "$1" ]] || fail "expected path to be absent: $1"
}

make_release() {
  local root="$1" version="$2" release_version
  case "$version" in
    v1) release_version=0.0.1 ;;
    v2) release_version=0.0.2 ;;
    v3) release_version=0.0.3 ;;
    *) release_version=0.0.9 ;;
  esac
  mkdir -p "$root/bin" "$root/shell/external" "$root/shell/vendor" "$root/web/assets"
  printf '#!/usr/bin/env bash\necho sentinal-%s\n' "$version" >"$root/bin/sentinal"
  printf '#!/usr/bin/env bash\necho sentinald-%s\n' "$version" >"$root/bin/sentinald"
  chmod 755 "$root/bin/sentinal" "$root/bin/sentinald"
  printf 'zsh hook %s\n' "$version" >"$root/shell/external/sentinal.zsh"
  printf 'bash hook %s\n' "$version" >"$root/shell/external/sentinal.bash"
  printf 'fish hook %s\n' "$version" >"$root/shell/external/sentinal.fish"
  printf 'pty zsh %s\n' "$version" >"$root/shell/ta.zsh"
  printf 'pty bash %s\n' "$version" >"$root/shell/ta.bash"
  printf 'pty fish %s\n' "$version" >"$root/shell/ta.fish"
  printf 'vendor %s\n' "$version" >"$root/shell/vendor/bash-preexec.sh"
  printf '<main>web %s</main>\n' "$version" >"$root/web/index.html"
  printf '{}\n' >"$root/web/manifest.webmanifest"
  printf 'const release = "%s";\n' "$version" >"$root/web/sw.js"
  printf 'console.log("%s")\n' "$version" >"$root/web/assets/app-test.js"
  cat >"$root/BUILD.json" <<EOF
{"schema_version":1,"version":"$release_version","git_commit":"test","source_date_epoch":1,"target":"aarch64-apple-darwin","protocol_version":1}
EOF
}

install_worker() {
  local platform="$1" release="$2" prefix="$3" config="$4" data="$5" service_log="$6"
  SENTINAL_INSTALL_PLATFORM="$platform" \
  SENTINAL_SERVICE_TEST_LOG="$service_log" \
    "$installer" install \
      --release-dir "$release" \
      --prefix "$prefix" \
      --config-dir "$config" \
      --data-dir "$data" \
      --bind 100.90.80.70 \
      --port 17420
}

# GNU stat accepts `-f` as filesystem mode, writes filesystem details, and
# then rejects the BSD format operand. A failed platform probe must not leak
# that stdout into the owner/mode result used by the safety check.
fake_stat_bin="$test_root/fake GNU stat"
mkdir -p "$fake_stat_bin"
cat >"$fake_stat_bin/stat" <<EOF
#!/usr/bin/env bash
if [[ "\${1:-}" == -f ]]; then
  echo 'unexpected filesystem details'
  exit 1
fi
if [[ "\${1:-}" == -c ]]; then
  echo '$(id -u) 755'
  exit 0
fi
exit 2
EOF
chmod 755 "$fake_stat_bin/stat"
owner_mode="$(
  PATH="$fake_stat_bin:$PATH" bash -c \
    'source "$1"; sentinal_stat_owner_mode "$2"' _ \
    "$repo_root/scripts/lib/install-common.sh" "$repo_root/Cargo.toml"
)"
[[ "$owner_mode" == "$(id -u) 755" ]] || \
  fail "GNU stat fallback leaked probe output: $owner_mode"

release="$test_root/release v1"
prefix="$test_root/install prefix"
config="$test_root/config retained"
data="$test_root/data retained"
service_log="$test_root/service-actions.log"
make_release "$release" v1

fresh_output="$(install_worker macos "$release" "$prefix" "$config" "$data" "$service_log")"
assert_contains "$fresh_output" "Installed sentinal and sentinald in $prefix/bin"
assert_contains "$fresh_output" 'Worker bind: 100.90.80.70:17420'
[[ -x "$prefix/bin/sentinal" ]] || fail 'sentinal was not installed executable'
[[ -x "$prefix/bin/sentinald" ]] || fail 'sentinald was not installed executable'
assert_file_contains "$prefix/bin/sentinal" 'sentinal-v1'
assert_file_contains "$prefix/share/sentinal/shell/external/sentinal.zsh" 'zsh hook v1'
assert_file_contains "$prefix/share/sentinal/web/index.html" 'web v1'
assert_file_contains "$prefix/share/sentinal/release.json" '"version": "0.0.1"'
assert_file_contains "$config/daemon.toml" 'bind = "100.90.80.70:17420"'
assert_file_contains "$config/daemon.toml" "data_dir = \"$data\""
if config_mode="$(stat -f '%Lp' "$config/daemon.toml" 2>/dev/null)"; then
  :
else
  config_mode="$(stat -c '%a' "$config/daemon.toml")"
fi
[[ "$config_mode" == 600 ]] || fail 'daemon config is not mode 0600'

mac_service="$prefix/service/dev.sentinal.sentinald.plist"
[[ -f "$mac_service" ]] || fail 'launchd service was not rendered'
assert_file_contains "$mac_service" "<string>$prefix/bin/sentinald</string>"
assert_file_contains "$mac_service" "<string>$config/daemon.toml</string>"
assert_file_contains "$service_log" "bootstrap gui/$(id -u) $mac_service"
assert_file_contains "$service_log" "wait launchd bootout gui/$(id -u)/dev.sentinal.sentinald"
if grep -Eq 'token[^_]|password|auth\.json' "$mac_service"; then
  fail 'launchd service contains a credential or credential path'
fi

mkdir -p "$data/sessions" "$data/adapters" "$data/recipes"
printf 'machine-token\n' >"$config/token"
printf 'password-hash\n' >"$config/auth.json"
printf 'sqlite\n' >"$data/sentinal.db"
printf 'output\n' >"$data/sessions/1.log.zst"
printf 'adapter\n' >"$data/adapters/custom.toml"
printf 'recipe\n' >"$data/recipes/custom.toml"
printf '# retained operator setting\n' >>"$config/daemon.toml"

make_release "$release" v2
reinstall_output="$(install_worker macos "$release" "$prefix" "$config" "$data" "$service_log")"
assert_contains "$reinstall_output" 'Preserved existing daemon config'
assert_file_contains "$prefix/bin/sentinal" 'sentinal-v2'
assert_file_contains "$prefix/share/sentinal/web/index.html" 'web v2'
assert_file_contains "$config/daemon.toml" '# retained operator setting'
assert_file_contains "$config/token" 'machine-token'
assert_file_contains "$config/auth.json" 'password-hash'
assert_file_contains "$data/sentinal.db" 'sqlite'
assert_file_contains "$data/sessions/1.log.zst" 'output'
assert_file_contains "$data/adapters/custom.toml" 'adapter'
assert_file_contains "$data/recipes/custom.toml" 'recipe'
assert_file_contains "$prefix/share/sentinal/release.json" '"version": "0.0.2"'
if find "$prefix/bin" -maxdepth 1 -name '*.tmp.*' | grep -q .; then
  fail 'atomic replacement left a temporary binary'
fi

rollback_release="$test_root/rollback release"
make_release "$rollback_release" v1
if output="$(
  SENTINAL_INSTALL_PLATFORM=macos SENTINAL_SERVICE_TEST_LOG="$service_log" \
    "$installer" install --release-dir "$rollback_release" --prefix "$prefix" \
      --config-dir "$config" --data-dir "$data" --bind 100.90.80.70 2>&1
)"; then
  fail 'implicit downgrade was accepted'
fi
assert_contains "$output" 'refusing downgrade from 0.0.2 to 0.0.1'
assert_file_contains "$prefix/bin/sentinal" 'sentinal-v2'

SENTINAL_INSTALL_PLATFORM=macos SENTINAL_SERVICE_TEST_LOG="$service_log" \
  "$installer" install --release-dir "$rollback_release" --prefix "$prefix" \
    --config-dir "$config" --data-dir "$data" --bind 100.90.80.70 \
    --rollback 0.0.1 >/dev/null
assert_file_contains "$prefix/bin/sentinal" 'sentinal-v1'
assert_file_contains "$prefix/share/sentinal/web/index.html" 'web v1'
assert_file_contains "$prefix/share/sentinal/release.json" '"version": "0.0.1"'

# A service failure restores the previous programs and release metadata.
make_release "$release" v3
if output="$(
  SENTINAL_INSTALL_PLATFORM=macos SENTINAL_SERVICE_TEST_LOG="$service_log" \
  SENTINAL_SERVICE_TEST_FAIL_ON='bootstrap' \
    "$installer" install --release-dir "$release" --prefix "$prefix" \
      --config-dir "$config" --data-dir "$data" --bind 100.90.80.70 2>&1
)"; then
  fail 'service failure unexpectedly completed installation'
fi
assert_contains "$output" 'service failed to start; restored 0.0.1'
assert_file_contains "$prefix/bin/sentinal" 'sentinal-v1'
assert_file_contains "$prefix/share/sentinal/release.json" '"version": "0.0.1"'
assert_file_contains "$prefix/share/sentinal/web/index.html" 'web v1'

linux_prefix="$test_root/linux prefix"
linux_config="$test_root/linux config"
linux_data="$test_root/linux data"
linux_log="$test_root/linux-service.log"
install_worker linux "$release" "$linux_prefix" "$linux_config" "$linux_data" "$linux_log" >/dev/null
linux_service="$linux_prefix/service/sentinald.service"
assert_file_contains "$linux_service" "ExecStart=\"$linux_prefix/bin/sentinald\""
assert_file_contains "$linux_service" "Environment=\"TA_CONFIG=$linux_config/daemon.toml\""
assert_file_contains "$linux_log" 'systemctl --user daemon-reload'
assert_file_contains "$linux_log" 'systemctl --user enable sentinald.service'
assert_file_contains "$linux_log" 'systemctl --user restart sentinald.service'
if grep -Eq 'token[^_]|password|auth\.json' "$linux_service"; then
  fail 'systemd service contains a credential or credential path'
fi

dry_prefix="$test_root/dry prefix"
dry_output="$(
  SENTINAL_INSTALL_PLATFORM=macos "$installer" install \
    --release-dir "$release" \
    --prefix "$dry_prefix" \
    --config-dir "$test_root/dry config" \
    --data-dir "$test_root/dry data" \
    --bind 100.100.1.2 \
    --dry-run
)"
assert_contains "$dry_output" 'DRY RUN'
assert_contains "$dry_output" "$dry_prefix/bin/sentinald"
assert_absent "$dry_prefix"

source_output="$(
  SENTINAL_INSTALL_PLATFORM=linux "$installer" install \
    --source-dir "$repo_root" \
    --prefix "$test_root/source dry" \
    --config-dir "$test_root/source config" \
    --data-dir "$test_root/source data" \
    --bind 100.100.1.3 \
    --no-service \
    --dry-run
)"
assert_contains "$source_output" 'cargo build --locked --offline --release'
assert_contains "$source_output" 'Service installation: disabled'
assert_contains "$source_output" "Add $test_root/source dry/bin to PATH"

if output="$(
  SENTINAL_INSTALL_PLATFORM=linux "$installer" install \
    --source-dir "$repo_root" \
    --release-dir "$release" \
    --prefix "$test_root/conflicting sources" \
    --config-dir "$test_root/conflicting config" \
    --data-dir "$test_root/conflicting data" \
    --bind 100.100.1.4 \
    --dry-run 2>&1
)"; then
  fail 'conflicting source and release directories were accepted'
fi
assert_contains "$output" 'use only one of --source-dir and --release-dir'

if output="$(SENTINAL_INSTALL_PLATFORM=plan9 "$installer" install --release-dir "$release" --prefix "$test_root/bad" --config-dir "$test_root/bad-cfg" --data-dir "$test_root/bad-data" --bind 100.90.1.1 2>&1)"; then
  fail 'unsupported platform was accepted'
fi
assert_contains "$output" "unsupported operating system 'plan9'"

incomplete="$test_root/incomplete"
make_release "$incomplete" broken
rm -rf "$incomplete/shell"
if output="$(SENTINAL_INSTALL_PLATFORM=macos "$installer" install --release-dir "$incomplete" --prefix "$test_root/incomplete-out" --config-dir "$test_root/incomplete-cfg" --data-dir "$test_root/incomplete-data" --bind 100.90.1.2 --no-service 2>&1)"; then
  fail 'release without shell assets was accepted'
fi
assert_contains "$output" "missing shell assets"

unsafe="$test_root/unsafe"
make_release "$unsafe" unsafe
chmod 777 "$unsafe/bin/sentinald"
if output="$(SENTINAL_INSTALL_PLATFORM=macos "$installer" install --release-dir "$unsafe" --prefix "$test_root/unsafe-out" --config-dir "$test_root/unsafe-cfg" --data-dir "$test_root/unsafe-data" --bind 100.90.1.3 --no-service 2>&1)"; then
  fail 'world-writable source binary was accepted'
fi
assert_contains "$output" "unsafe source binary '$unsafe/bin/sentinald'"

unsafe_config_prefix="$test_root/unsafe config prefix"
unsafe_config_dir="$test_root/unsafe config dir"
mkdir -p "$unsafe_config_dir"
ln -s "$test_root/not-a-config" "$unsafe_config_dir/daemon.toml"
if output="$(
  SENTINAL_INSTALL_PLATFORM=macos "$installer" install \
    --release-dir "$release" \
    --prefix "$unsafe_config_prefix" \
    --config-dir "$unsafe_config_dir" \
    --data-dir "$test_root/unsafe config data" \
    --bind 100.90.1.4 \
    --no-service 2>&1
)"; then
  fail 'symlink daemon config was accepted'
fi
assert_contains "$output" "existing daemon config '$unsafe_config_dir/daemon.toml' is not a regular file"
assert_absent "$unsafe_config_prefix/bin/sentinal"
assert_absent "$unsafe_config_prefix/bin/sentinald"

if output="$(
  SENTINAL_INSTALL_PLATFORM=macos SENTINAL_SERVICE_TEST_LOG="$service_log" \
    "$installer" uninstall \
      --prefix "$prefix" \
      --config-dir "$config" \
      --data-dir "$data" \
      --delete-data "$test_root/not-the-data-dir" 2>&1
)"; then
  fail 'mismatched data deletion confirmation was accepted'
fi
assert_contains "$output" "data deletion confirmation must exactly equal '$data'"
[[ -x "$prefix/bin/sentinald" ]] || fail 'refused uninstall changed program files'
[[ -f "$data/sentinal.db" ]] || fail 'refused uninstall changed retained data'

uninstall_output="$(
  SENTINAL_INSTALL_PLATFORM=macos SENTINAL_SERVICE_TEST_LOG="$service_log" \
    "$installer" uninstall \
      --prefix "$prefix" \
      --config-dir "$config" \
      --data-dir "$data"
)"
assert_contains "$uninstall_output" "Preserved config: $config"
assert_contains "$uninstall_output" "Preserved data: $data"
assert_absent "$prefix/bin/sentinal"
assert_absent "$prefix/bin/sentinald"
assert_absent "$prefix/share/sentinal"
assert_absent "$mac_service"
[[ -f "$config/token" ]] || fail 'uninstall removed retained token'
[[ -f "$data/sentinal.db" ]] || fail 'uninstall removed retained database'
assert_file_contains "$service_log" "bootout gui/$(id -u)/dev.sentinal.sentinald"

delete_prefix="$test_root/delete prefix"
delete_config="$test_root/delete config"
delete_data="$test_root/delete data"
delete_log="$test_root/delete-service.log"
install_worker macos "$release" "$delete_prefix" "$delete_config" "$delete_data" "$delete_log" >/dev/null
printf 'important\n' >"$delete_data/sentinal.db"
SENTINAL_INSTALL_PLATFORM=macos SENTINAL_SERVICE_TEST_LOG="$delete_log" \
  "$installer" uninstall \
    --prefix "$delete_prefix" \
    --config-dir "$delete_config" \
    --data-dir "$delete_data" \
    --delete-data "$delete_data" >/dev/null
assert_absent "$delete_data"
[[ -d "$delete_config" ]] || fail 'explicit data deletion also removed config'

echo 'worker installer test: passed'
