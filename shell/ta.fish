# terminal-agent fish integration.
function __ta_b64
    printf '%s' "$argv[1]" | base64 | string collect | string replace -a '\n' ''
end

function __ta_cwd
    printf '\e]7;file://%s%s\a' (hostname) "$PWD"
end

function __ta_preexec --on-event fish_preexec
    printf '\e]7420;cmd=%s\a' (__ta_b64 "$argv[1]")
    printf '\e]133;B\a\e]133;C\a'
end

function __ta_postexec --on-event fish_postexec
    set -l command_status $status
    printf '\e]133;D;%d\a' $command_status
    __ta_cwd
    printf '\e]133;A\a'
end

__ta_cwd
printf '\e]133;A\a'
