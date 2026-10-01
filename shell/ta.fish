# terminal-agent fish integration.
# Every signal carries the per-Session nonce so program output cannot forge one.
set -g __ta_nonce "$SENTINAL_OSC_NONCE"
set -e SENTINAL_OSC_NONCE

function __ta_b64
    printf '%s' "$argv[1]" | base64 | string collect | string replace -a '\n' ''
end

function __ta_signal
    printf '\e]%s;sentinal=%s\a' "$argv[1]" "$__ta_nonce"
end

function __ta_cwd
    printf '\e]7;file://%s%s\a' (hostname) "$PWD"
    __ta_signal "7420;cwd="(__ta_b64 "$PWD")
end

function __ta_preexec --on-event fish_preexec
    __ta_signal "7420;cmd="(__ta_b64 "$argv[1]")
    __ta_signal "133;B"
    __ta_signal "133;C"
end

function __ta_postexec --on-event fish_postexec
    set -l command_status $status
    __ta_signal "133;D;$command_status"
    __ta_cwd
    __ta_signal "133;A"
end

__ta_cwd
__ta_signal "133;A"
