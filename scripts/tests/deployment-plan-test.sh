#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

fail() {
  echo "deployment plan test failed: $*" >&2
  exit 1
}

require_text() {
  local file="$1" text="$2"
  grep -Fq -- "$text" "$file" || fail "expected $file to contain: $text"
}

bootstrap_prompt="$repo_root/prompts/03h-milestone-1c1-tailnet-bootstrap.md"
enrollment_prompt="$repo_root/prompts/05a-milestone-40-secure-machine-enrollment.md"

[[ -f "$bootstrap_prompt" ]] || fail 'the Milestone 1c.1 bootstrap prompt is missing'

require_text "$repo_root/docs/PLAN.md" 'install → UNPAIRED → connect → PAIRED'
require_text "$repo_root/docs/PLAN.md" '`sentinal connect`'
require_text "$repo_root/docs/ROADMAP.md" 'Milestone 1c.1 — Tailnet bootstrap and private package delivery'
require_text "$repo_root/docs/ROADMAP.md" 'Installation never enrolls or receives a pairing secret'
require_text "$repo_root/docs/ROADMAP.md" '`sentinal connect` pairs one installed worker'
require_text "$repo_root/docs/ARCHITECTURE.md" 'Machine token stays local'

require_text "$bootstrap_prompt" 'does **not** pair, enroll, name, or authorize the Machine'
require_text "$bootstrap_prompt" 'UNPAIRED'
require_text "$bootstrap_prompt" 'install.sh'
require_text "$bootstrap_prompt" 'release manifest'
require_text "$bootstrap_prompt" '100.64.0.0/10'

require_text "$enrollment_prompt" 'sentinal connect'
require_text "$enrollment_prompt" 'hidden TTY prompt'
require_text "$enrollment_prompt" 'Machine ID'
require_text "$enrollment_prompt" 'display name'
require_text "$enrollment_prompt" 'Machine token stays local'
require_text "$enrollment_prompt" 'must not appear in argv'

require_text "$repo_root/prompts/README.md" '03h-milestone-1c1-tailnet-bootstrap.md'

web_build_line="$(grep -nF '    pnpm -C web build' "$repo_root/justfile" | head -n 1 | cut -d: -f1 || true)"
installer_test_line="$(grep -nF '    bash scripts/tests/install-worker-test.sh' "$repo_root/justfile" | head -n 1 | cut -d: -f1 || true)"
[[ -n "$web_build_line" ]] || fail 'just check must build the production web bundle'
[[ -n "$installer_test_line" ]] || fail 'just check must run the worker installer test'
(( web_build_line < installer_test_line )) || \
  fail 'just check must build the production web bundle before installer validation'

echo 'deployment plan test: passed'
