#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
setup="$repo_root/docs/SETUP.md"

fail() {
  echo "setup documentation test failed: $*" >&2
  exit 1
}

require_text() {
  local file="$1" text="$2"
  grep -Fq -- "$text" "$file" || fail "expected $file to contain: $text"
}

[[ -f "$setup" ]] || fail 'docs/SETUP.md is missing'

for heading in \
  '## Prerequisites and Tailnet policy' \
  '## Install a worker' \
  '## First start and authentication' \
  '## Development launch roles' \
  '## Service operations' \
  '## Upgrade' \
  '## Backup and inspection' \
  '## Uninstall' \
  '## Two-host acceptance checklist' \
  '## Current boundaries'; do
  require_text "$setup" "$heading"
done

for command in \
  'just network' \
  'just agent' \
  'just control WORKER_IP=' \
  'just install-worker' \
  'sentinal watch' \
  'sentinal attach' \
  'systemctl --user status sentinald.service' \
  'launchctl print "gui/$(id -u)/dev.sentinal.sentinald"' \
  'just uninstall-worker'; do
  require_text "$setup" "$command"
done

require_text "$setup" '100.64.0.0/10'
require_text "$setup" '## Enroll a worker'
require_text "$setup" 'sentinal connect https://sentinal-control.TAILNET.ts.net'
require_text "$setup" 'Pairing code: [hidden input]'
require_text "$setup" 'allowed_origins'
require_text "$setup" 'service startup pairs implicitly'
require_text "$setup" 'FULL Sessions are interrupted'
require_text "$setup" 'metadata only'
require_text "$setup" 'Do not print or copy the token or password hash'

require_text "$repo_root/packaging/systemd/sentinald.service" 'reference template'
require_text "$repo_root/packaging/launchd/dev.sentinal.sentinald.plist" 'reference template'

if grep -Eiq 'token[[:space:]]*=|password[[:space:]]*=' \
  "$repo_root/packaging/systemd/sentinald.service" \
  "$repo_root/packaging/launchd/dev.sentinal.sentinald.plist"; then
  fail 'service reference templates contain credentials'
fi

echo 'setup documentation test: passed'
