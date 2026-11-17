# claude-hub

A TUI for Claude Code sessions and git repos in tmux.

- **Sessions view:** every Claude Code session with its status (waiting, working, idle, ended, stale).
- **Repos view:** git repos as a tree, with the tmux panes that live in each repo.

Claude Code hooks report session status. The TUI reads those status files and
`tmux list-panes`.

## Install

Requires Python 3.11+ and tmux.

```sh
python3 -m venv .venv
.venv/bin/pip install -e .
```

## Configure

Create `~/.config/claude-hub/config.toml`:

```toml
roots = ["~/repos"]      # scanned for git repos, max depth 2
mouse = true
refresh_seconds = 2
stale_minutes = 60       # sessions without a tmux pane go stale after this
```

All keys are optional. The default for `roots` is `~/code`.

## Install the hooks

```sh
.venv/bin/claude-hub install-hooks
```

This adds `claude-hub report` to the hooks in `~/.claude/settings.json`. The command
shows what it adds and asks first. It saves a backup as `settings.json.bak`.

Only Claude sessions started after this report status. Status files go to
`~/.local/state/claude-hub/`.

## Run

```sh
.venv/bin/claude-hub
```

As a tmux popup. `--popup` closes the popup after a jump:

```sh
tmux display-popup -E -w 85% -h 75% /path/to/claude-tui/.venv/bin/claude-hub --popup
```

A key binding in `~/.tmux.conf`:

```tmux
bind C display-popup -E -w 85% -h 75% /path/to/claude-tui/.venv/bin/claude-hub --popup
```

Use `--no-mouse` to turn off mouse support.

## Keys

| Key | Action |
|---|---|
| `1` / `2` / `Tab` | Switch view |
| `j` `k` / arrows | Move |
| `Enter` | Jump to the pane or repo session |
| `/` | Filter, `Esc` clears |
| `?` | Help |
| `q` | Quit |

Sessions view:

| Key | Action |
|---|---|
| `t` | New shell window in the session's repo |
| `r` | Show the session's repo in the Repos view |
| `x` | Remove ended and stale sessions |

Repos view:

| Key | Action |
|---|---|
| `Enter` | Jump to the repo's tmux session, or create it |
| `Space` / `l` / `h` | Expand / collapse |
| `c` / `e` / `t` | New window with claude / nvim / shell |
| `H` | Show or hide repos without panes |

Mouse: a click selects a row. A second click on the selected row (or a double-click)
works like `Enter`. Click `▸`/`▾` to expand, and click the tabs and footer keys.
