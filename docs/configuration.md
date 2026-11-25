# Configuration

Every key in `~/.config/jugglr/config.toml`, and how the list sections are chosen.

```toml
roots = ["~/repos"]          # where to look for git repos (any depth)
mouse = true
refresh_seconds = 2
stale_minutes = 60
prewarm = true               # keep a started shell ready per workspace
prewarm_claude = true        # keep started Claudes ready for the last selected workspaces
hub_key = "M-q"              # tmux key that brings you back to the hub from any terminal
theme = "dark-modern"        # dark-modern, light, catppuccin, gruvbox, tokyo-night, oxocarbon
web_theme = "jugglr"         # the web app: jugglr (follows the system), jugglr-light, jugglr-dark,
                             # dark-modern, light-modern, catppuccin, gruvbox, tokyo-night, oxocarbon
chat_dir = "~/chats"         # where C starts Claude for chats that belong to no repo
# The sections to show, in this order.
sections = ["workspaces", "gitlab-reviews", "gitlab-merge-requests",
            "gitlab-assigned-merge-requests", "gitlab-issues", "other-sessions", "closed-sessions"]
```

All keys are optional. The default for `roots` is `~/repos`. A section's title is its name
without `gitlab-`, in capitals: `gitlab-issues` is **ISSUES**. The hub fetches only the GitLab
sections in the list; leave them all out and it does not call `glab` for them. An unknown name is
skipped, and the hub shows a warning when it starts.

The hub reads the file when it starts. Run `jugglr restart` after a change.
