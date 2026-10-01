# terminal-agent zsh integration. Output is metadata only; it is never executed.
# Every signal carries the per-Session nonce so program output cannot forge one.
autoload -Uz add-zsh-hook

typeset -g _ta_nonce="${SENTINAL_OSC_NONCE-}"
unset SENTINAL_OSC_NONCE

_ta_b64() { printf '%s' "$1" | base64 | tr -d '\n'; }
_ta_signal() { printf '\033]%s;sentinal=%s\007' "$1" "$_ta_nonce"; }
_ta_cwd() {
  printf '\033]7;file://%s%s\007' "${HOST:-localhost}" "$PWD"
  _ta_signal "7420;cwd=$(_ta_b64 "$PWD")"
}

_ta_preexec() {
  _ta_signal "7420;cmd=$(_ta_b64 "$1")"
  _ta_signal "133;B"
  _ta_signal "133;C"
  typeset -g TA_COMMAND_ACTIVE=1
}

_ta_precmd() {
  local exit_code=$?
  if [[ -n "${TA_COMMAND_ACTIVE:-}" ]]; then
    _ta_signal "133;D;$exit_code"
    unset TA_COMMAND_ACTIVE
  fi
  _ta_cwd
  _ta_signal "133;A"
}

add-zsh-hook preexec _ta_preexec
add-zsh-hook precmd _ta_precmd
