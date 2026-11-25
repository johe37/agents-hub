<p align="center"><img src="docs/jugglr.png" alt="jugglr" width="160"></p>

# jugglr

One place for your Claude Code sessions and the terminals around them.

- **Workspaces.** A workspace is a directory with its Claude sessions, shells and editors. The
  hub lists them, the ones that need you first, with each agent's status and what it does now.
- **Status from hooks.** Claude Code hooks report each agent's status at once: working, waiting
  for you, or done.
- **GitLab.** Your merge requests with their review threads, the reviews you are asked for, and
  your issues, each in its own section.
- **Keyboard and mouse.** Vim-style keys, and the mouse works too.
- **Web app.** `jugglr web` opens the hub in the browser: an inbox to answer agents, chats,
  live terminals, diffs and notifications.
- **Keeps running.** Close the terminal, run `jugglr` again, and everything is where you
  left it.

## Install

Requires Linux, tmux, Rust (cargo) and Claude Code. The web app also needs Node.js (npm) to
build. `e` opens nvim, so install it if you want that key. The GitLab sections need [`glab`](https://gitlab.com/gitlab-org/cli), logged in with
`glab auth login`.

```sh
cargo install --path .
jugglr install-hooks
```

`install-hooks` shows what it changes in `~/.claude/settings.json` and asks first. Only Claude
sessions started after it report status.

To update, pull and run `cargo install --path .` again; the next `jugglr` restarts the hub
UI on the new binary.

## Get started

```sh
jugglr
```

Press `n` to open your first workspace, then `c` to start Claude in it.

| Key | Action |
|---|---|
| `j` / `k` | Move down / up |
| `Enter` | Go to the agent, terminal or workspace |
| `Alt+Q` | Back to the hub from any terminal |
| `n` | Open a workspace |
| `c` / `t` / `e` | New Claude / terminal / nvim in the workspace |
| `r` | Reply to the agent |
| `w` | Jump to the next agent waiting for you |
| `/` | Filter |
| `?` | Every key |
| `q` | Leave; everything keeps running |

## Read more

- [Using the hub](docs/usage.md): workspaces, agents, the panel, the picker, every key and the mouse
- [GitLab](docs/gitlab.md): merge requests, review threads, reviews and issues
- [The web app](docs/web.md): the hub in the browser
- [Configuration](docs/configuration.md): every setting, and which sections the list shows
- [How it works](docs/how-it-works.md): hooks, tmux, previews, restart and stop
- [Development](docs/development.md): tests, the demo and releases
