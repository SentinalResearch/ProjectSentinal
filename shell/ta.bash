# terminal-agent bash integration using the vendored bash-preexec dispatcher.
# Every signal carries the per-Session nonce so program output cannot forge one.
source "${TA_HOOK_DIR}/vendor/bash-preexec.sh"

_ta_nonce="${SENTINAL_OSC_NONCE-}"
unset SENTINAL_OSC_NONCE

_ta_b64() { printf '%s' "$1" | base64 | tr -d '\n'; }
_ta_signal() { printf '\033]%s;sentinal=%s\007' "$1" "$_ta_nonce"; }
_ta_cwd() {
  printf '\033]7;file://%s%s\007' "${HOSTNAME:-localhost}" "$PWD"
  _ta_signal "7420;cwd=$(_ta_b64 "$PWD")"
}

_ta_preexec() {
  _ta_signal "7420;cmd=$(_ta_b64 "$1")"
  _ta_signal "133;B"
  _ta_signal "133;C"
}

_ta_precmd() {
  local status="${1:-0}"
  _ta_signal "133;D;$status"
  _ta_cwd
  _ta_signal "133;A"
}

preexec_functions+=(_ta_preexec)
precmd_functions+=(_ta_precmd)
