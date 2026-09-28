# Minimal vendored bash-preexec-compatible hook dispatcher for terminal-agent.
# Exposes preexec_functions and precmd_functions arrays used by ta.bash.
declare -a preexec_functions
declare -a precmd_functions
__ta_preexec_ready=0

__ta_run_precmd() {
  local status=$?
  local fn
  for fn in "${precmd_functions[@]}"; do "$fn" "$status"; done
  __ta_preexec_ready=1
}

__ta_run_preexec() {
  [[ "$__ta_preexec_ready" == 1 ]] || return
  [[ "$BASH_COMMAND" == __ta_* ]] && return
  __ta_preexec_ready=0
  local fn
  for fn in "${preexec_functions[@]}"; do "$fn" "$BASH_COMMAND"; done
}

trap '__ta_run_preexec' DEBUG
PROMPT_COMMAND="__ta_run_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
