#!/usr/bin/env bash
# `just promote`: fast-forward main to dev once every automated check passes.
# Pushes nothing and releases nothing; `just ship X.Y.Z` is the separate last step.
set -euo pipefail
readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
die() { echo "promote: $*" >&2; exit 1; }

[[ "$(git rev-parse --abbrev-ref HEAD)" == dev ]] || die "run this from the dev branch"
[[ -z "$(git status --porcelain --untracked-files=no)" ]] || die "commit or stash your changes first"
git merge-base --is-ancestor main dev || die "main has commits dev lacks; bring them into dev first"
[[ "$(git rev-parse main)" != "$(git rev-parse dev)" ]] || { echo "main already matches dev."; exit 0; }

if [[ "${SENTINAL_PROMOTE_SKIP_CHECKS:-}" != 1 ]]; then
  just check
  just test-e2e
fi
tested="$(git rev-parse dev)"
[[ "$(git rev-parse HEAD)" == "$tested" && -z "$(git status --porcelain --untracked-files=no)" ]] ||
  die "the tree changed during the checks"
git fetch . dev:main
echo "main is now at $(git log -1 --format='%h %s' main)"
echo "Nothing was pushed or released. To release: git checkout main && just ship X.Y.Z"
