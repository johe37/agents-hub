# The web app

`jugglr web` runs the hub as a web app on your machine and opens it in the browser.
It shows the same workspaces and agents as the terminal hub, and both run at the same time.

```sh
jugglr web              # serve on 127.0.0.1:7420 and open the browser
jugglr web --port 8080  # another port
jugglr web --no-open    # print the link only
```

Stop it with `Ctrl+C`. Workspaces and agents keep running. If the hub runs already, the web app uses it as it
is: it does not restart it, also after an update.

## Install it as an app

In Chrome, open the page with the link that `jugglr web` printed. Then click the install icon in
the address bar, or open the menu and pick **Cast, save and share → Install page as app…**. The
app opens in a window of its own and gets an icon in the Dock and in Launchpad. It works while
`jugglr web` runs.

In Safari (macOS 14 or later), open the page with the link, then pick **File → Add to Dock…**.
Safari copies the page's cookies into the app when you add it, so open the link first. The app
keeps its own cookies after that: if you make a new token, remove the app and add it again.

The browser binds the app to the address, port included. Use the same `--port` each time.

To change the icons, replace the files in `web/public/icons/` and build again. Keep the sizes.
The maskable icons and `apple-touch-icon.png` fill the whole square; Chrome on macOS and
Safari cut them to a rounded square.

## What it does

- **Inbox.** Every agent that waits for you, with what it asks. A permission prompt shows the
  command or the diff of the edit, so you can approve or deny it there. Text you send instead
  of an answer declines the tool and goes to Claude as your next prompt. Below the inbox: the
  agents that work now, and the ones that finished a turn.
- **Activity.** One lane per agent over the last 1, 4, 8 or 24 hours: when it worked and when
  it waited for you. The graph above the lanes shows how many agents worked and waited at once.
  Hover a span for its times; click a lane to open the agent. The hook logs each status change to
  `~/.local/state/jugglr/history/`, so lanes start after you install this version. Logs that
  do not change for a week are removed.
- **Workspaces.** One tab per window. An agent opens in its terminal; `v` switches to the
  chat: its conversation, with the tool calls folded, and a box to reply. Shells are live terminals
  in the page; start your editor in one. The side panel lists the merge request and the
  changed files; click a file or a commit to see its diff in a diff tab after the windows.
  Each diff gets its own tab; opening one again shows its tab. `Esc` goes back to the window
  you came from.
- **Edits in the chat.** Each edit shows open as a diff with the file's line numbers. A turn that
  changed files ends with a list of them; click a file for its diff against HEAD. Click or
  Shift+click line numbers in an edit, then **Ask Claude**: the lines go to the reply box. Failed
  tool calls and Claude's latest to-do list also show open.
- **Open a workspace** (`o`). Pick a directory. If it has no workspace yet, choose what starts
  in it: Claude or a terminal. The workspace opens with only that window.
- **New agent** (`n`). Pick a directory and write the task. The workspace opens if it is not
  open yet, and Claude starts with the task as its first prompt.
- **New chat** (`Shift+C`). Ask Claude something that belongs to no repo. Claude starts in
  `chat_dir` (`~/chats` by default), as `C` does in the terminal hub. The sidebar lists the
  chats under **Chats**; click one to open it.
- **Closed.** The last section of the sidebar lists closed workspaces and chats, newest
  first. Click a workspace to open it again with its windows; click a chat to resume it.
- **Voice chat.** Click the microphone in a reply box and talk. When you stop, it is sent; when
  Claude finishes, the browser reads the answer aloud and listens again. While Claude asks for
  permission, say "yes" or "no". Click the microphone again to stop. Needs Chrome or Edge: it uses
  the browser's own speech recognition, which sends your voice to Google or Microsoft.
- **GitLab** (`g l`). Four tabs: **To-Do** (your GitLab To-Do list: mentions, reviews, failed,
  assigned, other), **To review** (new, in progress, approved), **My MRs** (needs you, waiting,
  ready to merge, drafts) and **Issues**; `1` to `4` switch, `/` filters, and a click on a group
  title folds it. The count in the menu is the number of pending To-Dos. A To-Do shows who did
  what above its MR or issue; `x` or **Done** marks it done in GitLab, and **Mark all done**
  (click twice) empties the list. Each item has a detail pane: the
  description, the open review threads and the MR's pipelines. Reply in a thread or resolve
  it. See [Reviewing a merge request](#reviewing-a-merge-request). "Ask Claude to fix" puts the
  thread in the reply box of the Claude in the side panel, to edit before you send it.
  **Open** checks out the MR's code in a workspace and starts Claude (`c`) or a terminal (`t`)
  there. When the workspace is open, the button goes to it, and its arrow adds a window. While
  the hub clones the project and checks out the code, the button shows the step and how far it
  has come. On an issue, `c` starts Claude in your clone with a prompt about the issue. A workspace on a branch shows its MR in the side panel, and a button to
  push (and open a draft MR when it has none). Checkouts work as in the terminal hub; see
  [gitlab.md](gitlab.md).
- **Command palette** (`Ctrl+K`). Go to any workspace or agent, open any repo, run any action.
- **Notifications.** Click the bell. The browser then tells you when an agent needs you or
  finishes, while the page is in the background. The tab title and icon show how many wait.
- **Themes.** Click the sun or moon in the sidebar, or run "Change theme…" in the palette.
  The theme under the cursor shows at once; `Enter` keeps it. The pick holds for this
  browser. Without a pick, the page uses `web_theme` from the config file; see
  [configuration.md](configuration.md).

`?` in the page lists every key.

## Reviewing a merge request

An MR's detail pane has four tabs: **Overview**, **Commits** (`l`), **Changes** (`d`) and
**Pipelines** (`p`). `Esc` goes back to the overview.

- **Changes** shows the diff as GitLab does, unified or side by side (`s`), with syntax
  colors. Pick one commit, or only the changes since your last review, in the list above the
  diff. A click on a commit in **Commits** also shows only its diff. `[` and `]` go to the
  previous and next file. Mark a file as viewed to fold it; it unfolds again when its diff
  changes.
- **Pipelines** shows the head pipeline first: a line on how it went, then its stages as
  columns from left to right. Click a job to read the end of its log, and again to close it;
  the log of a running job updates every few seconds. A failed job has its own **Log**,
  **Retry** and **Ask Claude**; the last asks the Claude in the MR's checkout to find the
  cause. **Retry** and **Cancel** above the stages act
  on the whole pipeline, and **Run pipeline** starts a new one. Below, **Earlier pipelines**
  lists the MR's older pipelines with their stages; click one to show its jobs and logs.
- **Select lines.** Press a line number and drag to the last line you want; near the top or
  bottom the diff scrolls. Let go, and a bar under the lines offers **Ask Claude** and
  **Comment**. A click selects one line, Shift+click selects up to another line in the same
  hunk, and `Esc` or **Cancel** clears the selection.
- **A whole file.** In a file's header, the ✦ button asks Claude about the file, and the
  speech bubble opens a comment on the file, like "Comment on this file" in GitLab. The comment
  shows at the top of the file.
- **Ask Claude** (`a`). The side panel opens with the Claude in the MR's checkout, and its
  reply box names the file, the lines and a git command that shows them. Write your question
  and press `Enter`. If no Claude runs there, the hub checks the code out and starts one.
- **Pair review** (`\`) in the MR's header shows or hides the side panel. The first time, it
  starts Claude in the MR's checkout with the review prompt, to edit before you send it. On
  your own MR, the button opens Claude without a prompt.
- **Claude's findings.** The review prompt asks Claude to end with its findings as a
  `review-findings` block. The chat shows one line for the block, and the findings show in the
  diff under their lines, with dashed borders. **Keep** adds a finding to your review as a
  draft comment, **Edit** opens it in the comment form first, **Ask** puts it in the side panel
  and **Dismiss** throws it away. The file list and the **Changes** tab count the findings that
  wait for you. A finding on a file the MR does not change goes in the review's summary when
  you keep it. This browser keeps the findings and what you did with them, also after Claude
  stops.
- **Comment** (`c`). **Add to review** keeps the comment as a draft in GitLab, like "Start a
  review" there. **Comment now** posts it at once. The comment shows in its place right away,
  marked "Sending…"; if GitLab refuses it, the form comes back with your text. Under an answer from Claude, **Comment with
  this** opens a comment with that text on the lines you asked about.
- **Finish review** publishes your drafts, with a summary if you write one, and approves the
  MR if you tick the box. **Approve** in the header approves without a review.

Comments go on all changes only: GitLab places them in the MR's latest version. The hub
remembers the head commit when you finish a review or approve, in
`~/.local/state/jugglr/reviewed.json`; "Since your last review" shows the diff from it.

## Terminals in the page

A terminal in the page is a tmux client of its own, in a hidden session grouped with the
workspace (`_web…`). So it can show another window than your terminal, and closing the tab
closes nothing. tmux sizes a window for the client that used it last, so a window you look
at in both places changes size when you switch between them.

`Ctrl+K` goes to the terminal program. Use `Ctrl+Shift+K` for the palette there.

The other shortcuts go to the terminal program too. Press `Ctrl+Shift+Space` first: the
page shows a menu of the main shortcuts, and the next key or a click in the menu runs one
(`c`, `t`, `[`, `w`, `g a` …). `Esc` or a click outside cancels it. `Alt+1–9` and
`Alt+Shift+←/→` switch windows from a terminal without the prefix.

## Security

The server listens on 127.0.0.1 only. The link it prints holds a token; opening it stores the
token in a cookie, and every request needs it. The token is in
`~/.local/state/jugglr/web-token`; delete the file to make a new one. The server also
refuses requests for other host names and from other sites.

Anyone with the token can type into your terminals. Do not share the link.

## Build

`cargo build` builds the page with npm (`web/`) and puts it in the binary. Without Node.js the
binary still builds, without the web app. To work on the page with live reload, run
`jugglr web --no-open` and, in `web/`, `npm run dev`.
