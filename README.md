# Agent Hub

One place for your Claude Code sessions and the terminals around them.

You work in **workspaces**. A workspace is a directory with its Claude sessions,
shells and editors. The directory does not have to be a git repo; when it is, the hub
shows branch, changes and ahead/behind.

Work keeps running when you leave. Close the terminal, come back later, run
`claude-hub` again and everything is where you left it. Under the hood claude-hub
runs a private tmux server (`tmux -L claude-hub`), so you never have to deal with tmux
yourself.

## Install

Requires Python 3.11+ and tmux.

```sh
python3 -m venv .venv
.venv/bin/pip install -e .
```

## Configure (optional)

`~/.config/claude-hub/config.toml`:

```toml
roots = ["~/repos"]          # where to look for git repos (any depth)
mouse = true
refresh_seconds = 2
stale_minutes = 60
```

All keys are optional. The default for `roots` is `~/repos`.

## Install the hooks

```sh
.venv/bin/claude-hub install-hooks
```

This adds `claude-hub report` to the hooks in `~/.claude/settings.json`. The command
shows what it adds and asks first. It saves a backup as `settings.json.bak`.

Only Claude sessions started after this report status.

## Run

```sh
.venv/bin/claude-hub
```

This starts the hub, or reconnects to it if it already runs. Press `n` to open your
first workspace.

Like `goto`, it also takes an argument:

```sh
claude-hub .              # open a workspace for this directory (or jump to it)
claude-hub claude tui     # open the hub with the picker searching for "claude tui"
```

In the picker (`n`):

- Type words to fuzzy search repos, directories and recent places.
- Type a path (`~/`, `/`, `./`) to browse directories like in a shell. `Tab` completes
  the selected directory and lists what is inside it. `→` accepts the grey suggestion.
- `Enter` opens it (a new workspace starts with a terminal), `Ctrl+O` starts Claude instead.

The directory list is cached in `~/.cache/claude-hub/dirs.tsv`. Press `Ctrl+R` in the
picker after you clone a new repo.

| Where | Key | Action |
|---|---|---|
| Workspace | Click **◀ Claude Hub** | Back to the hub (or tmux prefix + `s`, pick `_hub`) |
| Hub | `Enter` | Open the workspace or window |
| Hub | `n` | Open a workspace: fuzzy search or type a path |
| Hub | `c` / `t` / `e` | New Claude / shell / nvim window in the workspace |
| Hub | `x` | Close window or workspace |
| Hub | `R` | Rename |
| Hub | `Space` / `l` / `h` | Expand / collapse |
| Hub | `/` | Filter, `Esc` clears |
| Hub | `q` | Leave. Everything keeps running |

Inside a workspace, the bar at the top shows where you are: a **◀ Claude Hub** button (click it
to go back), the workspace name and path, its windows (click to switch) and how many
Claude sessions wait for you. When you enter a workspace, a short message reminds you
how to get back.

Opening a directory that already has a workspace jumps to it instead of creating a
second one.

Claude sessions that run outside claude-hub show up under "Outside claude-hub" with
their status.

Mouse: a click selects a row. A second click on the selected row (or a double-click)
opens it. Click `▸`/`▾` to expand.

## Notes

- The hub's tmux server loads your own `~/.tmux.conf` first, so your prefix and key
  bindings work. The hub then sets its own status bar and mouse settings on top.
- Running `claude-hub` restarts the hub UI when the code has changed since it started.

- `claude-hub ui` runs only the hub UI in the current terminal, without the tmux engine.
- To stop everything: `tmux -L claude-hub kill-server`.
