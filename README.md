# Agents Hub

One place for your Claude Code sessions and the terminals around them.

The hub lists your **workspaces**, the ones that need you first. A workspace is a directory
with its Claude sessions, shells and editors. Under each workspace you see its **agents**
(every running Claude session, titled by its task, with status and what it does right now)
and its terminals. The panel next to the list shows what the selected agent waits for, its
last message, the changed files and the terminals in the workspace.

The directory does not have to be a git repo; when it is, the hub shows branch, changes and
ahead/behind.

The panel follows the selected row. A workspace shows what is open in it. An agent shows what
it waits for, your last request and its last reply. A terminal shows its screen. Terminal
previews are rendered in color: the hub streams each terminal's
output to `~/.local/state/claude-hub/screens/` and replays it, because `tmux capture-pane`
crashes some tmux builds. A terminal opened before the hub started piping it shows its
screen from the next output on.

Work keeps running when you leave. Close the terminal, come back later, run
`claude-hub` again and everything is where you left it. Under the hood claude-hub
runs a private tmux server (`tmux -L claude-hub`), so you never have to deal with tmux
yourself.

## Install

Requires Rust (cargo) and tmux.

```sh
cargo install --path .
```

This puts `claude-hub` in `~/.cargo/bin`. Run `cargo install --path .` again after
pulling changes; the next `claude-hub` restarts the hub UI on the new binary.

## Configure (optional)

`~/.config/claude-hub/config.toml`:

```toml
roots = ["~/repos"]          # where to look for git repos (any depth)
mouse = true
refresh_seconds = 2
stale_minutes = 60
prewarm = true               # keep a started shell ready per workspace
hub_key = "C-q"              # tmux key that brings you back to the hub from any terminal
```

All keys are optional. The default for `roots` is `~/repos`.

## Install the hooks

```sh
claude-hub install-hooks
```

This adds `claude-hub report` to the hooks in `~/.claude/settings.json`. It replaces
older claude-hub hooks, including the `hook.sh` of the Python version. The command shows what it
changes and asks first. It saves a backup as `settings.json.bak`.

The hook runs on every Claude tool call, so it does almost nothing: it stores the latest
event per session (about 3 ms) and the hub picks it up at once.

Only Claude sessions started after this report status.

## Run

```sh
claude-hub
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
| Anywhere | `Ctrl+Q` or click **◀ Claude Hub** | Back to the hub |
| Hub | `Enter` | Go to the agent, the terminal or the workspace |
| Hub | `n` | Open a workspace: fuzzy search or type a path |
| Hub | `c` / `t` / `e` | New Claude / terminal / nvim in the workspace |
| Hub | `x` | Close the agent or the workspace |
| Hub | `R` | Rename the workspace |
| Hub | `r` | Reply: type a message, `Enter` sends it to the agent |
| Hub | `y` / `d` | Approve / deny a permission prompt |
| Hub | `PgUp` / `PgDn` | Scroll the detail panel |
| Hub | `/` | Filter, `Esc` clears |
| Hub | `q` | Leave. Everything keeps running |

Inside a workspace, the bar at the top shows where you are: a **◀ Claude Hub** button (click it
or press `Ctrl+Q` to go back), the workspace name and path, its windows (click to switch) and
how many Claude sessions wait for you.

Opening a directory that already has a workspace jumps to it instead of creating a
second one.

`Enter` on an agent takes you to it: plain Claude Code in its terminal. `Ctrl+Q` brings you
back. Ctrl+Q is free in Claude Code, zsh and nvim, and it is typeable on every keyboard layout. `Esc` cannot do that job: Claude, nvim and the shell all use it themselves.

Quick answers do not need the trip. Press `r`, type a message and press `Enter`: the hub types
it into the agent's terminal for you. When an agent asks for permission, `y` approves and `d`
denies, and the panel shows the tool call it wants to run.

Claude sessions that run outside claude-hub are listed too, marked *outside hub*. The hub
shows their status and messages and can reply to them, but cannot jump to them.

Mouse: a click selects a row. A second click on the selected row (or a double-click)
opens it. Click a terminal in the detail panel to jump to it.

The title of an agent comes from its transcript: the title Claude Code gives the session,
or the first prompt until then. The last message, the last request, the model and the
cost come from the same file.

## Notes

- The hub's tmux server loads your own `~/.tmux.conf` first, so your prefix and key
  bindings work. The hub then sets its own status bar and mouse settings on top.
- Running `claude-hub` restarts the hub UI when the binary has changed since it started.
- New terminals open instantly because the hub keeps one started shell per workspace in
  a hidden `_pool` session, and starts one for the directory you highlight in the picker.
  Turn it off with `prewarm = false`.

- `claude-hub ui` runs only the hub UI in the current terminal, without the tmux engine.
- To stop everything: `tmux -L claude-hub kill-server`.
