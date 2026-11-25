# The web app

`claude-hub web` runs the hub as a web app on your machine and opens it in the browser.
It shows the same workspaces and agents as the terminal hub, and both run at the same time.

```sh
claude-hub web              # serve on 127.0.0.1:7420 and open the browser
claude-hub web --port 8080  # another port
claude-hub web --no-open    # print the link only
```

Stop it with `Ctrl+C`. Workspaces and agents keep running.

## What it does

- **Inbox.** Every agent that waits for you, with what it asks. A permission prompt shows the
  command or the diff of the edit, so you can approve or deny it there. Text you send instead
  of an answer declines the tool and goes to Claude as your next prompt. Below the inbox: the
  agents that work now, and the ones that finished a turn.
- **Workspaces.** One tab per window. An agent opens as a chat: its conversation, with the tool
  calls folded, and a box to reply. Switch to its terminal with `v`. Shells and editors are
  live terminals in the page. The side panel lists the agents, the merge request and the
  changed files; click a file to see its diff.
- **New agent** (`n`). Pick a directory and write the task. The workspace opens if it is not
  open yet, and Claude starts with the task as its first prompt.
- **Voice chat.** Click the microphone in a reply box and talk. When you stop, it is sent; when
  Claude finishes, the browser reads the answer aloud and listens again. While Claude asks for
  permission, say "yes" or "no". Click the microphone again to stop. Needs Chrome or Edge: it uses
  the browser's own speech recognition, which sends your voice to Google or Microsoft.
- **Command palette** (`Ctrl+K`). Go to any workspace or agent, open any repo, run any action.
- **Notifications.** Click the bell. The browser then tells you when an agent needs you or
  finishes, while the page is in the background. The tab title and icon show how many wait.

`?` in the page lists every key.

## Terminals in the page

A terminal in the page is a tmux client of its own, in a hidden session grouped with the
workspace (`_web…`). So it can show another window than your terminal, and closing the tab
closes nothing. tmux sizes a window for the client that used it last, so a window you look
at in both places changes size when you switch between them.

`Ctrl+K` goes to the terminal program. Use `Ctrl+Shift+K` for the palette there.

## Security

The server listens on 127.0.0.1 only. The link it prints holds a token; opening it stores the
token in a cookie, and every request needs it. The token is in
`~/.local/state/claude-hub/web-token`; delete the file to make a new one. The server also
refuses requests for other host names and from other sites.

Anyone with the token can type into your terminals. Do not share the link.

## Build

`cargo build` builds the page with npm (`web/`) and puts it in the binary. Without Node.js the
binary still builds, without the web app. To work on the page with live reload, run
`claude-hub web --no-open` and, in `web/`, `npm run dev`.
