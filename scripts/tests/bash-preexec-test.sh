#!/usr/bin/env bash
# The bash dispatcher Sentinal loads into its own Sessions (shell/vendor/
# bash-preexec.sh) must report the line as the person typed it: not with
# aliases expanded, and not only the first command of a pipeline or list.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cat >"$work/rc" <<RC
source "$root/shell/vendor/bash-preexec.sh"
record() { printf '%s\n' "\$1" >>"$work/recorded"; }
preexec_functions+=(record)
alias ls='ls -d'
HISTFILE="$work/history"
HISTCONTROL=ignorespace
RC
printf 'old command from yesterday\n' >"$work/history"

# A leading space with ignorespace is not in history, so the fallback is used,
# never yesterday's last line.
printf '%s\n' \
  ' echo first >/dev/null' \
  'ls . >/dev/null' \
  'echo a | grep a >/dev/null' \
  'true && echo done >/dev/null' \
  ' echo hidden >/dev/null' \
  'exit' | env -i HOME="$work" PATH="$PATH" TERM=dumb "${BASH_UNDER_TEST:-bash}" --rcfile "$work/rc" -i >/dev/null 2>&1

expected='echo first > /dev/null
ls . >/dev/null
echo a | grep a >/dev/null
true && echo done >/dev/null
echo hidden > /dev/null
exit'
actual="$(cat "$work/recorded")"
if [[ "$actual" != "$expected" ]]; then
  printf 'expected:\n%s\n\nrecorded:\n%s\n' "$expected" "$actual" >&2
  exit 1
fi
echo "bash-preexec-test: ok"
