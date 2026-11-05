#!/usr/bin/env bash
# `just promote` moves main only forward, only from a clean dev, and pushes nothing.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-promote-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
fail() { echo "promote-dev test failed: $*" >&2; exit 1; }

repo="$test_root/repo"
git init -q -b main "$repo"
cd "$repo"
git config user.email test@example.invalid
git config user.name test
mkdir scripts
cp "$repo_root/scripts/promote-dev.sh" scripts/
git add scripts && git commit -qm base
export SENTINAL_PROMOTE_SKIP_CHECKS=1
run() { bash scripts/promote-dev.sh 2>&1; }

if output="$(run)"; then fail "ran from main"; fi
[[ "$output" == *"from the dev branch"* ]] || fail "wrong refusal on main: $output"

git checkout -qb dev
[[ "$(run)" == *"already matches"* ]] || fail "nothing to promote was not noticed"

echo work >file && git add file && git commit -qm work
echo dirty >file
if output="$(run)"; then fail "promoted a dirty tree"; fi
[[ "$output" == *"commit or stash"* ]] || fail "wrong refusal when dirty: $output"
git checkout -q -- file

git remote add origin "$test_root/no-such-remote"
output="$(run)" || fail "a clean dev was refused: $output"
[[ "$(git rev-parse main)" == "$(git rev-parse dev)" ]] || fail "main did not move"
[[ "$output" == *"Nothing was pushed"* ]] || fail "missing the release note"
[[ "$(git rev-parse --abbrev-ref HEAD)" == dev ]] || fail "left the dev branch"

git checkout -q main && echo hotfix >hot && git add hot && git commit -qm hotfix
git checkout -q dev
if output="$(run)"; then fail "promoted over a commit main had"; fi
[[ "$output" == *"main has commits dev lacks"* ]] || fail "wrong refusal on divergence: $output"
echo "promote-dev test: passed"
