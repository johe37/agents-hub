# Configuration

Every key in `~/.config/jugglr/config.toml`, and how the list sections are chosen.

```toml
agent = "claude"             # the command that starts an agent: claude, codex, gemini, aider, …
roots = ["~/repos"]          # where to look for git repos (any depth)
mouse = true
refresh_seconds = 2
stale_minutes = 60
prewarm = true               # keep a started shell ready per workspace
prewarm_claude = true        # keep started Claudes ready for the last selected workspaces (Claude only)
hub_key = "M-q"              # tmux key that brings you back to the hub from any terminal
theme = "dark-modern"        # dark-modern, light, catppuccin, gruvbox, tokyo-night, oxocarbon
web_theme = "jugglr"         # the web app: jugglr (follows the system), jugglr-light, jugglr-dark,
                             # dark-modern, light-modern, catppuccin, gruvbox, tokyo-night, oxocarbon
chat_dir = "~/chats"         # where C starts the agent for chats that belong to no repo
# The sections to show, in this order.
sections = ["workspaces", "gitlab-reviews", "gitlab-merge-requests",
            "gitlab-assigned-merge-requests", "gitlab-issues", "other-sessions", "closed-sessions"]
```

All keys are optional. The default for `roots` is `~/repos`. A section's title is its name
without `gitlab-`, in capitals: `gitlab-issues` is **ISSUES**. The hub fetches only the GitLab
sections in the list; leave them all out and it does not call `glab` for them. An unknown name is
skipped, and the hub shows a warning when it starts.

The hub reads the file when it starts. Run `jugglr restart` after a change.

## Other agents than Claude Code

`agent` is the command that `c`, `C`, new agents, reviews and issues start. Change it with `A` in
the hub or under Settings → Agent in the web app. Both save it in this file, and the hub and the
web app use the new value at once, without a restart. The command can have arguments, like
`codex --full-auto`.

Claude Code is the only agent with full support. The others:

- show as working while their screen changes, and idle when it is still, as they send no hooks;
- show their screen in the side panel instead of a chat, and `i` types into it;
- get a prompt typed in once their screen is still, instead of as an argument;
- are not pre-started (`prewarm_claude`), resumed or adopted.

The hub finds an agent's window by its name, which is the program's name, e.g. `codex`. If the
program is not in `PATH`, the hub hides the agent actions and says why.
