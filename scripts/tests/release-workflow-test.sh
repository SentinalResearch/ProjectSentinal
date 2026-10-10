#!/usr/bin/env bash
# Static safety checks for the GitHub Actions release workflow.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$repo_root/.github/workflows/release.yml"

fail() {
  echo "release workflow test failed: $*" >&2
  exit 1
}

require_text() {
  grep -Fq -- "$1" "$workflow" || fail "expected the workflow to contain: $1"
}

[[ -f "$workflow" ]] || fail "missing $workflow"

# Only an exact version tag starts a release; never a branch, PR, or fork.
require_text 'tags: ["v[0-9]+.[0-9]+.[0-9]+"]'
if grep -Eq 'pull_request|workflow_run|branches:' "$workflow"; then
  fail 'releases must be started only by a version tag'
fi

# Every third-party action is pinned to a full commit SHA.
while IFS= read -r line; do
  [[ "$line" =~ uses:\ [A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}( |$) ]] ||
    fail "action not pinned to a commit SHA: ${line#"${line%%[![:space:]]*}"}"
done < <(grep -E '^\s*-?\s*uses:' "$workflow")

# Least privilege: read-only by default; write scopes only where needed.
require_text $'permissions:\n  contents: read'
[[ "$(grep -c 'contents: write' "$workflow")" == 1 ]] || fail 'exactly one job may write repository contents'
[[ "$(grep -c 'id-token: write' "$workflow")" == 1 ]] || fail 'exactly one job may mint an OIDC token'
require_text 'attestations: write'
require_text 'persist-credentials: false'
if grep -Eq 'secrets\.' "$workflow"; then
  fail 'the release workflow must not use repository secrets'
fi

# Every supported target is built, attested, and checksummed.
targets="$(sed -n 's/^ *\([a-z0-9_]*-[a-z0-9_-]*\)) printf .*/\1/p' "$repo_root/scripts/lib/package-common.sh")"
[[ -n "$targets" ]] || fail 'could not read supported targets from package-common.sh'
for target in $targets; do
  require_text "target: $target"
done
require_text 'actions/attest-build-provenance@'
require_text 'SHA256SUMS'
require_text 'scripts/build-worker-release.sh'
require_text '--frozen-lockfile'
require_text 'cargo fetch --locked'

# A human reviews the draft before anything is published.
require_text '--draft'
require_text '--verify-tag'

echo 'release workflow test: passed'
