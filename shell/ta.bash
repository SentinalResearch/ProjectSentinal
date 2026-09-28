# terminal-agent bash integration using the vendored bash-preexec dispatcher.
source "${TA_HOOK_DIR}/vendor/bash-preexec.sh"

_ta_b64() { printf '%s' "$1" | base64 | tr -d '\n'; }
_ta_cwd() { printf '\033]7;file://%s%s\007' "${HOSTNAME:-localhost}" "$PWD"; }

_ta_preexec() {
  printf '\033]7420;cmd=%s\007' "$(_ta_b64 "$1")"
  printf '\033]133;B\007\033]133;C\007'
}

_ta_precmd() {
  local status="${1:-0}"
  printf '\033]133;D;%d\007' "$status"
  _ta_cwd
  printf '\033]133;A\007'
}

preexec_functions+=(_ta_preexec)
precmd_functions+=(_ta_precmd)
