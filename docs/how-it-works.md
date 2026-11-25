# How it works

Hooks, the private tmux server, terminal previews, the started shells and Claudes, and the
commands that restart or stop the hub.

## Status from hooks

The hub gets the status of each agent from Claude Code hooks:

```sh
jugglr install-hooks
```

This adds `jugglr report` to the hooks in `~/.claude/settings.json`. It replaces older
hooks from claude-hub, the old name of jugglr, and the `hook.sh` of the Python version. The
command shows what it changes and asks first. It saves a backup as `settings.json.bak`.

The hook runs on every Claude tool call, so it does almost nothing: it stores the latest
event per session (about 3 ms) and the hub picks it up at once.

Only Claude sessions started after this report status.

The command also sets `jugglr statusline` as the Claude Code status line, if you have
none. Claude Code passes your plan usage only to the status line. The hub then shows the
5-hour and weekly limits at the bottom right, with the time until each resets. This needs
a claude.ai subscription, and the numbers come after the first reply in any session. If you
already have a status line, it stays; send its JSON input to `jugglr statusline` too.

## tmux

jugglr runs a private tmux server (`tmux -L jugglr`), so you never have to deal with
tmux yourself. Workspaces are tmux sessions on it, so they keep running when you close the
terminal. Run `jugglr` again and everything is where you left it.

## Terminal previews

The panel renders a terminal's screen in color: the hub streams each terminal's output to
`~/.local/state/jugglr/screens/` and replays it, because `tmux capture-pane` crashes some
tmux builds. A terminal opened before the hub started piping it shows its screen from the
next output on.

## Notes

- The hub's tmux server loads your own `~/.tmux.conf` first, so your prefix and key
  bindings work. The hub then sets its own status bar and mouse settings on top.
- Running `jugglr` restarts the hub UI when the binary has changed since it started.
- New terminals open instantly because the hub keeps one started shell per workspace in
  a hidden `_pool` session, and starts one for the directory you highlight in the picker.
  Turn it off with `prewarm = false`.
- `c` opens Claude instantly the same way: the hub starts a Claude in `_pool` for the
  selected workspace, or for the directory highlighted in the picker, so `Ctrl+O` is
  instant too. Claude itself needs a few seconds to start, so press `c` a moment after you
  select the workspace. The hub keeps Claudes for the last two workspaces you selected,
  and starts a new one as soon as you take one. It replaces each every 10 minutes or when
  the branch changes, so the git state and CLAUDE.md it read are fresh; the old one stays
  ready until the new one has started. It stops them when nobody has looked at the hub
  for 15 minutes.
  Turn it off with `prewarm_claude = false`.
- `jugglr ui` runs only the hub UI in the current terminal, without the tmux engine.

## Restart or stop

- `jugglr restart` restarts the hub UI, for example when it froze. Workspaces and
  agents keep running.
- `jugglr stop` stops the tmux engine. This closes all workspaces with their agents
  and terminals, so it asks first. Add `-y` to skip the question.
- To stop the status reports, remove the `jugglr report` hooks from
  `~/.claude/settings.json`.

## Moving from claude-hub

jugglr was called claude-hub before. The first `jugglr` run moves `~/.local/state/claude-hub`,
`~/.cache/claude-hub`, `~/.config/claude-hub` and the tmux socket to their `jugglr` names. It
leaves a symlink at each old path, so a running claude-hub engine and its agents keep working.

1. Run `cargo install --path .`, then `jugglr install-hooks`. It replaces the
   `claude-hub` hooks and status line.
2. Run `jugglr`. Your workspaces are still there.
3. Remove the old binary: `cargo uninstall claude-hub`.
4. When no old agent runs any more, remove the symlinks.
