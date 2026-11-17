#!/bin/sh
# Claude Code hook for claude-hub. Runs on every hook event, so it only stores the
# latest event per session and leaves all parsing to the hub. It uses shell builtins
# only (apart from cat), because every forked process adds milliseconds to each tool
# call. Always exits 0: a failing PreToolUse hook would block Claude's tool call.
dir="${XDG_STATE_HOME:-$HOME/.local/state}/claude-hub/events"
input=$(cat)
case $input in *'"session_id"'*) ;; *) exit 0 ;; esac
id=${input#*\"session_id\"}
id=${id#*\"}
id=${id%%\"*}
case $id in "" | *[!A-Za-z0-9_-]*) exit 0 ;; esac
[ -d "$dir" ] || mkdir -p "$dir" || exit 0
# The hub skips a file it reads mid-write and picks it up on the next refresh.
printf '{"time":%s,"tmux":"%s","tmux_pane":"%s","event":%s}\n' \
    "${EPOCHSECONDS:-$(date +%s)}" "${TMUX%%,*}" "${TMUX_PANE:-}" "$input" > "$dir/$id.json"
exit 0
