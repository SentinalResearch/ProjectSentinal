# terminal-agent zsh integration. Output is metadata only; it is never executed.
autoload -Uz add-zsh-hook

_ta_b64() { printf '%s' "$1" | base64 | tr -d '\n'; }
_ta_cwd() { printf '\033]7;file://%s%s\007' "${HOST:-localhost}" "$PWD"; }

_ta_preexec() {
  printf '\033]7420;cmd=%s\007' "$(_ta_b64 "$1")"
  printf '\033]133;B\007\033]133;C\007'
  typeset -g TA_COMMAND_ACTIVE=1
}

_ta_precmd() {
  local exit_code=$?
  if [[ -n "${TA_COMMAND_ACTIVE:-}" ]]; then
    printf '\033]133;D;%d\007' "$exit_code"
    unset TA_COMMAND_ACTIVE
  fi
  _ta_cwd
  printf '\033]133;A\007'
}

add-zsh-hook preexec _ta_preexec
add-zsh-hook precmd _ta_precmd
