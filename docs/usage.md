# Using the hub

Workspaces, agents, the panel, the picker, every key and the mouse. GitLab has its own page:
[gitlab.md](gitlab.md).

## Workspaces and agents

The hub lists your **workspaces**, the ones that need you first. A workspace is a directory
with its Claude sessions, shells and editors. Under each workspace you see its **agents**
(every running Claude session, titled by its task, with status and what it does right now)
and its terminals. The panel next to the list shows what the selected agent waits for, its
last message, the changed files and the terminals in the workspace.

The directory does not have to be a git repo; when it is, the hub shows branch, changes and
ahead/behind.

The panel follows the selected row. A workspace shows what is open in it. An agent shows what
it waits for, your last request and its last reply. A terminal shows its screen, in color.
A terminal opened before the hub started shows its screen from the next output on.

## Open a workspace

## Run

```sh
jugglr
```

This starts the hub, or reconnects to it if it already runs. Press `n` to open your
first workspace.

Like `goto`, it also takes an argument:

```sh
jugglr .          # open a workspace for this directory (or jump to it)
jugglr claude tui # open the hub with the picker searching for "claude tui"
```

In the picker (`n`):

- Type words to fuzzy search repos, directories and recent places.
- Type a path (`~/`, `/`, `./`) to browse directories like in a shell. `Tab` completes
  the selected directory and lists what is inside it. `→` accepts the grey suggestion.
- `Enter` opens it (a new workspace starts with a terminal), `Ctrl+O` starts Claude instead.

The directory list is cached in `~/.cache/jugglr/dirs.tsv`. Press `Ctrl+R` in the
picker after you clone a new repo.

## Keys

| Where | Key | Action |
|---|---|---|
| Anywhere | `Alt+Q` or click **◀ Jugglr** | Back to the hub |
| Hub | `j` / `k` or `↓` / `↑` | Move down / up. A count first moves that many rows: `4j` |
| Hub | `J` / `K` | Jump to the next / previous workspace, review or section |
| Hub | `g` / `G`, `Ctrl+B` / `Ctrl+F`, `Ctrl+U` / `Ctrl+D` | First / last row, page up / down, half a page up / down |
| Hub | `Enter` | Go to the agent, the terminal or the workspace |
| Hub | `n` | Open a workspace: fuzzy search or type a path |
| Hub | `c` / `t` / `e` | New Claude / terminal / nvim in the workspace |
| Hub | `C` or click **chat** | New chat: Claude in the `chats` workspace (`chat_dir`), for questions that belong to no repo. `claude --resume` there lists earlier chats |
| Hub | `x` | Close the agent or the workspace |
| Hub | `R` | Rename the workspace |
| Hub | `r` | Reply: type a message, `Enter` sends it to the agent |
| Hub | `y` / `d` | Approve / deny a permission prompt |
| Hub | `Space` or click `▸`/`▾` | Fold or unfold the workspace, merge request or section. Agents that wait for you still show. **Workspaces** and **Other sessions** start open; the GitLab sections and **Closed sessions** start folded |
| Hub | `z` | Fold every workspace and every section except **Workspaces**, or unfold them all when all are folded |
| Hub | `m` | Mark the agent as done, so it stops showing as waiting |
| Hub | `a` | Adopt an agent that runs outside the hub |
| Hub | `w` | Jump to the next agent waiting for you |
| Hub | `h` `l` / `←` `→` / `Tab` or `1`–`4` | Switch the tab in the detail panel. A digit waits 0.4 s for a `j`/`k` before it switches |
| Thread | `Enter` / `r` / `f` | Open in GitLab / reply in the thread / put it in Claude's prompt |
| Thread | `v` | Resolve the thread, posting an optional reply first (pre-filled with `Fixed in <commit>.`) |
| Hub | `p` | Push the workspace's branch; opens a draft merge request when it has none |
| Hub | `PgUp` / `PgDn` | Scroll the detail panel |
| Hub | `/` | Filter, `Esc` clears |
| Hub | `?` | Help |
| Hub | `q` | Leave. Everything keeps running |

## Inside a workspace

Inside a workspace, the bar at the top shows where you are: a **◀ Jugglr** button (click it
or press `Alt+Q` to go back), the workspace name and path, its windows (click to switch) and
how many Claude sessions wait for you.

Opening a directory that already has a workspace jumps to it instead of creating a
second one.

`Enter` on an agent takes you to it: plain Claude Code in its terminal. `Alt+Q` brings you
back. Alt+Q is free in Claude Code and nvim, and it is easy to reach with the left hand. (In zsh it shadows `push-line`.) `Esc` cannot do that job: Claude, nvim and the shell all use it themselves.

Quick answers do not need the trip. Press `r`, type a message and press `Enter`: the hub types
it into the agent's terminal for you. When an agent asks for permission, `y` approves and `d`
denies, and the panel shows the tool call it wants to run.

## Mouse

Mouse: a click selects a row. A second click on the selected row (or a double-click)
opens it. Click a terminal in the detail panel to jump to it.

## Sessions outside the hub

Claude sessions that run outside jugglr are listed under **OTHER SESSIONS**, marked *outside hub*. The hub
shows their status and messages and can reply to them, but cannot jump to them. Press `a`
to adopt one: the hub stops the process and resumes the session with `claude --resume` in
a new window of the workspace for its directory.

## Agent titles

The title of an agent comes from its transcript: the title Claude Code gives the session,
or the first prompt until then. The last message, the last request, the model and the
cost come from the same file.
