# Minimal vendored bash-preexec-compatible hook dispatcher for terminal-agent.
# Exposes preexec_functions and precmd_functions arrays used by ta.bash.
declare -a preexec_functions
declare -a precmd_functions
__ta_preexec_ready=0

__ta_run_precmd() {
  local status=$?
  local fn
  for fn in "${precmd_functions[@]}"; do "$fn" "$status"; done
  # The newest history entry before the next line, so a line history does not
  # record is never taken for the one before it.
  local entry
  entry=$(HISTTIMEFORMAT= builtin history 1)
  [[ $entry =~ ^[[:space:]]*([0-9]+) ]] && __ta_histnum=${BASH_REMATCH[1]}
  __ta_preexec_ready=1
  return "$status"
}

__ta_histnum=

# The line as typed, from history: $BASH_COMMAND has aliases expanded and holds
# only the first simple command of a pipeline or list. A line history did not
# record (ignorespace, ignoredups, history off) falls back to $BASH_COMMAND.
__ta_typed_line() {
  local entry number text=$BASH_COMMAND
  entry=$(HISTTIMEFORMAT= builtin history 1)
  if [[ $entry =~ ^[[:space:]]*([0-9]+)[*]?[[:space:]]+(.*)$ ]]; then
    number=${BASH_REMATCH[1]}
    [[ $number != "$__ta_histnum" ]] && text=${BASH_REMATCH[2]}
  fi
  __ta_line=$text
}

__ta_run_preexec() {
  [[ "$__ta_preexec_ready" == 1 ]] || return
  [[ "$BASH_COMMAND" == __ta_* ]] && return
  __ta_preexec_ready=0
  local __ta_line fn
  __ta_typed_line
  for fn in "${preexec_functions[@]}"; do "$fn" "$__ta_line"; done
}

trap '__ta_run_preexec' DEBUG
PROMPT_COMMAND="__ta_run_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
