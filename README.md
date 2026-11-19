# Agents Hub

One place for your Claude Code sessions and the terminals around them.

![The hub with four workspaces, merge requests and one agent waiting for permission](docs/screenshot.png)

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

Requires Linux (the hub reads `/proc`), tmux, Rust (cargo) and Claude Code. `e` opens
nvim, so install it if you want that key.

```sh
cargo install --path .
```

This puts `claude-hub` in `~/.cargo/bin`. Run `cargo install --path .` again after
pulling changes; the next `claude-hub` restarts the hub UI on the new binary.

### Install the hooks

The hub gets the status of each agent from Claude Code hooks:

```sh
claude-hub install-hooks
```

This adds `claude-hub report` to the hooks in `~/.claude/settings.json`. It replaces
older claude-hub hooks, including the `hook.sh` of the Python version. The command shows what it
changes and asks first. It saves a backup as `settings.json.bak`.

The hook runs on every Claude tool call, so it does almost nothing: it stores the latest
event per session (about 3 ms) and the hub picks it up at once.

Only Claude sessions started after this report status.

The command also sets `claude-hub statusline` as the Claude Code status line, if you have
none. Claude Code passes your plan usage only to the status line. The hub then shows the
5-hour and weekly limits at the bottom right, with the time until each resets. This needs
a claude.ai subscription, and the numbers come after the first reply in any session. If you
already have a status line, it stays; send its JSON input to `claude-hub statusline` too.

## Configure (optional)

`~/.config/claude-hub/config.toml`:

```toml
roots = ["~/repos"]          # where to look for git repos (any depth)
mouse = true
refresh_seconds = 2
stale_minutes = 60
prewarm = true               # keep a started shell ready per workspace
prewarm_claude = true        # keep a started Claude ready for the selected workspace
hub_key = "M-q"              # tmux key that brings you back to the hub from any terminal
theme = "dark-modern"        # dark-modern, light, catppuccin, gruvbox, tokyo-night
```

All keys are optional. The default for `roots` is `~/repos`. The XP keys are under
[XP, levels and loot](#xp-levels-and-loot).

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
| Anywhere | `Alt+Q` or click **◀ Claude Hub** | Back to the hub |
| Hub | `j` / `k` or `↓` / `↑` | Move down / up |
| Hub | `g` / `G`, `Ctrl+B` / `Ctrl+F` | First / last row, page up / down |
| Hub | `Enter` | Go to the agent, the terminal or the workspace |
| Hub | `n` | Open a workspace: fuzzy search or type a path |
| Hub | `c` / `t` / `e` | New Claude / terminal / nvim in the workspace |
| Hub | `x` | Close the agent or the workspace |
| Hub | `R` | Rename the workspace |
| Hub | `r` | Reply: type a message, `Enter` sends it to the agent |
| Hub | `y` / `d` | Approve / deny a permission prompt |
| Hub | `Space` or click `▸`/`▾` | Fold or unfold the workspace or section. Agents that wait for you still show. **Local workspaces** starts open; **Merge requests to review** and **Recently closed Claude sessions** start folded |
| Hub | `z` | Fold every workspace, **Merge requests to review** and **Recently closed Claude sessions**, or unfold them all when all are folded |
| Hub | `m` | Mark the agent as done, so it stops showing as waiting |
| Hub | `a` | Adopt an agent that runs outside the hub |
| Hub | `w` | Jump to the next agent waiting for you |
| Hub | `h` `l` / `←` `→` / `Tab` or `1`–`4` | Switch the tab in the detail panel |
| Thread | `Enter` / `r` / `f` | Open in GitLab / reply in the thread / put it in Claude's prompt |
| Thread | `v` | Resolve the thread, posting an optional reply first (pre-filled with `Fixed in <commit>.`) |
| Hub | `p` | Push the workspace's branch; opens a draft merge request when it has none |
| Hub | `PgUp` / `PgDn` | Scroll the detail panel |
| Hub | `/` | Filter, `Esc` clears |
| Hub | `?` | Help |
| Hub | `q` | Leave. Everything keeps running |

Inside a workspace, the bar at the top shows where you are: a **◀ Claude Hub** button (click it
or press `Alt+Q` to go back), the workspace name and path, its windows (click to switch) and
how many Claude sessions wait for you.

Opening a directory that already has a workspace jumps to it instead of creating a
second one.

`Enter` on an agent takes you to it: plain Claude Code in its terminal. `Alt+Q` brings you
back. Alt+Q is free in Claude Code and nvim, and it is easy to reach with the left hand. (In zsh it shadows `push-line`.) `Esc` cannot do that job: Claude, nvim and the shell all use it themselves.

Quick answers do not need the trip. Press `r`, type a message and press `Enter`: the hub types
it into the agent's terminal for you. When an agent asks for permission, `y` approves and `d`
denies, and the panel shows the tool call it wants to run.

**Merge requests.** When a workspace's branch has a GitLab merge request, its row shows
`!140 ✓  2`: the MR, its pipeline (✓ passed, ✗ failed, ⟳ running) and its unresolved review
threads. Each thread is listed under the workspace. Select one to read it, `r` to reply in
GitLab, `f` to put it in the workspace's Claude prompt and jump there (edit it, then `Enter` sends), `Enter` to open it in the browser. This
uses `glab`, so run `glab auth login` once. MR data refreshes every minute in the background,
every 10 seconds while a pipeline runs. `Ctrl+R` on a workspace fetches it right away.

To close the loop: `f` sends a comment to Claude, `p` pushes the fix (and opens a draft MR the
first time), `v` resolves the thread with a "Fixed in <commit>" reply you can edit. The **Pipeline** tab (`4`) shows each stage and job
with its status and duration.

**Reviewing others' merge requests.** MRs where you are a requested reviewer are listed under
**MERGE REQUESTS TO REVIEW**, newest activity first, with author, pipeline and open threads. The panel shows the
description, threads, approvals and pipeline. `Enter` opens the MR in GitLab. `c` checks out its
code in a separate git worktree of your local clone (under `~/.cache/claude-hub/reviews/`),
opens it as a workspace and starts Claude with a review prompt typed in but not sent: adjust it,
then press `Enter`. Your own checkout is never touched. `x` on a review workspace closes it and
deletes the checkout. The list refreshes every two minutes; `Ctrl+R` on it refreshes now.
The list is fetched outside any repo, so `glab` asks its default host. If your GitLab is not
gitlab.com, set it once, or the list stays empty: `glab config set --global host git.example.com`.

Links in the panel (the `!140` line of an MR, "Open the thread in GitLab") are real terminal
hyperlinks: Ctrl+click (Cmd+click on macOS) opens them in the browser of the computer you sit
at, also over SSH.

Claude sessions that run outside claude-hub are listed too, marked *outside hub*. The hub
shows their status and messages and can reply to them, but cannot jump to them. Press `a`
to adopt one: the hub stops the process and resumes the session with `claude --resume` in
a new window of the workspace for its directory.

Mouse: a click selects a row. A second click on the selected row (or a double-click)
opens it. Click a terminal in the detail panel to jump to it.

The title of an agent comes from its transcript: the title Claude Code gives the session,
or the first prompt until then. The last message, the last request, the model and the
cost come from the same file.

## Town view

```sh
claude-hub town            # opens the town in your browser
claude-hub town --demo     # a made-up town and player, to see what it looks like
```

The same data as the hub, drawn as a small pixel art town. Every git repo is a shop, and so
is every directory outside a repo. Agents in the same repo share a shop, whether they run in
the hub, in your own tmux or in a plain terminal; subdirectories and worktrees count as their
repo. A shop grows a counter space for each agent and never moves once it has a lot.

| You see | It means |
|---|---|
| Someone typing at the counter, a tool icon over the shop (✎ edit, 🔍 search, ⚙ bash…) | Working |
| Someone at the door with a blinking **!**, and a **!** over the roof | Needs your permission |
| A **?** bubble at the counter | Waits for your reply |
| Head on the counter, **z z z** | Done with its turn, or stale |
| Someone walking the street with a letter | Taking a message to another agent |

On the shop front, crates mean uncommitted changes, parcels right of the shop are commits to
push and parcels left of the door are commits to pull. A shop with a plain wooden awning and
no crates or parcels is not a git repo. Smoking chimneys are terminals running something,
lamp posts are open editors, and a lit window means someone works there. Hover a shop for its
branch.

Click a shop or a person to open the panel. It is the hub's own detail panel, built by the
same code and in the same colours, so it shows what the hub shows: what the agent waits for,
your last request, its last reply, changed files, windows, model and cost. `y`, `d`, `r` and
`m` work there as in the hub, and `1`–`4` or `h`/`l` switch tabs.

To start a new Claude in a repo, select its shop and press `c` (or click **new Claude here**).
To start one anywhere else, press `c` (or `n`) with no shop selected, or click **+ New Claude**:
search your repos, recent places and folders like the hub's picker, or type a path (`~/`, `/`).
In a folder Claude has not worked in before, the town asks first: Claude's safety check would
otherwise wait unseen in the new window, and its default answer (*No, exit*) closes Claude. Press
`Enter` again or click **Trust and start**, and the hub answers the check with *Yes, I trust this
folder* for you. A first task is sent once Claude is ready; with `claude-hub install-hooks` done
that is as soon as Claude reports in, without hooks once its screen has settled.
Type a first task, or leave it empty, and press `Enter`: Claude starts in a new window of the repo's
workspace (the workspace is made if there is none), and the task is sent as soon as Claude is ready.
The new agent shows up behind the counter; `Alt+Q` in the hub or the workspace's bar takes you to it. The counter in the corner
shows how many agents wait for you; click it to go to the next one. Drag to move, scroll to
zoom. The town follows your clock, and dark mode makes it night.

The server listens on 127.0.0.1 only. The page gets a random token in its URL, and the
server refuses requests without it or from other origins, so other web pages cannot see
your agents or answer them. Options: `--port N`, `--no-open`.

### XP, levels and loot

Closing GitLab issues levels you up. Every closed issue that is **assigned to you** gives XP,
also the ones you closed before you installed claude-hub: its weight times `xp_per_weight`. An
issue without a weight counts as weight 1 (`default_weight`). Who closed it
does not matter, and an issue you closed but were not assigned to gives nothing. Your open
assigned issues are your **quests**.

XP fills the bar at the bottom of the town; hover it for the issues that gave the latest XP. Each
level needs more XP than the last (`100 × level^1.5`). On a level-up your character jumps and a
random piece of loot drops, with an item level equal to your new level.

Each drop is rolled in three steps:

1. **Slot**: head, chest, hands, feet, main hand, off-hand, back or trinket, each equally likely.
2. **Rarity**: Poor, Common, Uncommon, Rare, Epic or Legendary, from `drop_chances`.
3. **Item**: a kind that fits the slot (the off-hand can be a shield, mousepad, mug or keyboard;
   the head a helmet or headphones) and its look; a legendary is one of the slot's legendaries
   you do not have yet.

- **Name**: the rarity's word and the item: *Broken* (Poor), *Working* (Common), *Good*
  (Uncommon), *Excellent* (Rare) or *Masterwork* (Epic) Mousepad, Helm, Mug, Keyboard… Only legendaries have names of their own.
- **Stats** (Focus, Speed, Insight, Stamina) add up to a total set by item level and rarity: item
  level + 1 for Poor, times Common 1.2, Uncommon 1.9, Rare 2.5, Epic 3.2 or Legendary 4. A Poor
  item at level 1 has 2 (say 1 Speed and 1 Stamina); at level 10 a Poor item has 11 and an Epic 35.
  The total is spread at random over up to 2 stats (Poor, Common), 3 (Uncommon, Rare) or 4 (Epic,
  Legendary). Stats are only for show.

- **Legendaries**: three for every slot, each with its own name, a line of lore and a glow, such as
  *Rimecrown*, *The Last Merge* or *The Heisenmug*. Each drops once. They grow with you: their item
  level is always your level, and their stats follow the legendary total for it. Once you own all
  three for a slot, a legendary roll for that slot gives an Epic instead.

Each drop is rolled once and kept in your progress file.

Your **class** comes from the label you have closed the most issues with. Only these labels
count, matched exactly (ignoring case); every other label is ignored:

| Label | Class |
|---|---|
| `Severity::Critical` (or `Critical`) | Beacon Warden, the first responder |
| `Type::Tech Debt` (or `Tech Debt`) | Debtreaver, the debt collector |
| `Low hanging fruit` | Orchard Forager, the fruit picker |
| `Type::Bug` (or `Bug`) | Verminbane, the exterminator |
| `Type::Enhancement` (or `Enhancement`) | Enchanter, the enhancer |
| `Documentation` | Lorescribe, the scribe |

A tie goes to the class you closed an issue for most recently. Before your first such issue you
are a Wanderer. The character sheet shows your class and how many issues you closed for each.

The left side of the town is yours. `p` opens your character: drag loot from the bag onto its slot,
or double-click it, to wear it. Hover an item to compare it with what you wear. `i` opens the bag,
`q` the quest log; a scroll over a shop means one of your quests is in that repo. Your character
walks the streets and goes to the door of any shop where an agent waits for you. Click it to open
the character sheet. On the first visit you choose how it looks; **Change look** changes it later.
The hub's footer shows your level too.

**Setup.** XP comes through `glab`, so claude-hub never handles your token:

1. Make a personal access token in GitLab (*Preferences → Access tokens*) with the `read_api`
   scope. That is all XP needs; other hub features, like replying to review threads, need `api`.
2. Log glab in with it: `glab auth login --hostname git.example.com`. Add `--use-keyring` to keep
   the token in the OS keyring instead of glab's config file. Or export `GITLAB_TOKEN` in your
   shell profile; glab reads that first. Never put the token in claude-hub's config.
3. Tell claude-hub which GitLab to ask, and sync once:

```toml
gitlab_url = "https://git.example.com"  # your GitLab; without it glab's default host is used
# gitlab_username = "me"                # default: whoever glab is logged in as
xp_per_weight = 10                      # XP per weight point
default_weight = 1                      # an issue without a weight counts as this
sync_minutes = 15                       # how often the hub and the town sync in the background
xp_since = "all"                        # "all", "install", or a date like "2026-01-01"
sound = false                           # level-up and loot sounds in the town

[drop_chances]                          # relative weights of the rarities
poor = 20
common = 40
uncommon = 25
rare = 10
epic = 4
legendary = 1
```

```sh
claude-hub xp sync     # fetch now and show what you got
claude-hub xp          # your level, XP and when it last synced
```

With `gitlab_url` (or `gitlab_username`) set, the hub and the town sync in the background every
`sync_minutes`. A failed sync (no network, GitLab down, rate limited) changes nothing: the town
keeps your last known XP and tries again later; hover the XP bar to see the error.

Progress is in `~/.local/state/claude-hub/progress.json`: every counted issue, your XP, level,
loot, what you wear and your look. Everything works offline from it.

How issues count:

- Each issue counts once, by its global id. Reopened and closed again: no new XP.
- The weight it had when it first counted stays, even if it changes later.
- Unassigned after it closed and counted: you keep the XP. Unassigned before the hub saw it: none.
- Issues moved to another project do not count (GitLab closes the original); the moved copy
  counts when it closes.
- `xp_since = "all"` (the default) counts every closed issue assigned to you, from your whole
  history. `"install"` counts only issues closed after the progress file was made, a date counts
  from that day. Changing it fetches again from the new start; issues already counted keep their XP.
- Changing `xp_per_weight` or `default_weight` changes XP for issues closed from then on, and the
  rewards shown on quests.

### Agents talking to each other

```sh
claude-hub send <agent-or-workspace> "Can you check the login tests?"
```

types the message into that agent, the way the hub's reply does. The target is a session id
(or its first characters), a workspace, repo or directory name, or an agent's title. Run by an
agent, the message starts with who sent it and how to answer, and in the town the sender walks
over to the other shop, says it at the door and walks back. An agent running
`tmux send-keys -t <pane>` to another agent's pane counts too. Each message is stored in
`~/.local/state/claude-hub/messages/` for a day. `send` refuses an agent that waits for a
permission answer, so a message never approves anything.

### Claude in your own tmux, nvim or a plain terminal

The hub finds every Claude, not only those in its own workspaces, and can type into one when
its tmux pane is Claude's own terminal. Claude in an nvim `:terminal` works too, in or out of
tmux: keys sent to the pane would go to nvim, so the hub writes to that terminal through nvim's
RPC socket (`$NVIM`) instead. That reaches the right Claude whatever nvim shows and whichever
mode it is in, also with several Claudes in one nvim. Claude inside other programs in a pane
(screen, ssh) and in plain terminals outside tmux is read-only, with the reason in the panel.

```sh
claude-hub debug-sessions
```

lists every Claude session the hub finds: pid, directory, tmux socket and pane, whether its
hook has reported, and whether the hub can type into it, and if not, why. If it says the hook
never reported, run `claude-hub install-hooks` and start Claude again.

## Notes

- The hub's tmux server loads your own `~/.tmux.conf` first, so your prefix and key
  bindings work. The hub then sets its own status bar and mouse settings on top.
- Running `claude-hub` restarts the hub UI when the binary has changed since it started.
- New terminals open instantly because the hub keeps one started shell per workspace in
  a hidden `_pool` session, and starts one for the directory you highlight in the picker.
  Turn it off with `prewarm = false`.
- `c` opens Claude instantly the same way: the hub starts one Claude in `_pool` for the
  selected workspace, or for the directory highlighted in the picker, so `Ctrl+O` is
  instant too. Claude itself needs a few seconds to start, so press `c` a moment after you
  select the workspace. The hub keeps only that one Claude, restarts it every
  10 minutes or when the branch changes, so the git state and CLAUDE.md it read are fresh,
  and stops it when nobody has looked at the hub for 15 minutes.
  Turn it off with `prewarm_claude = false`.
- `claude-hub ui` runs only the hub UI in the current terminal, without the tmux engine.
- To stop everything: `tmux -L claude-hub kill-server`.

## Screenshot demo

`scripts/demo.sh` starts a hub with made-up workspaces, agents and merge requests, for
screenshots. It runs on its own tmux server with its own HOME, so your real hub is not
touched. Run it outside tmux and it attaches at once. `scripts/demo.sh stop` removes it.

## Release

`scripts/release.sh 0.6.0` sets the version in `Cargo.toml`, commits and tags. Claude writes
release notes from the commits since the last tag and the tag keeps them.
Then push with `git push origin master 0.6.0`. The push of the tag starts a GitHub workflow
that creates the GitHub release with these notes.
