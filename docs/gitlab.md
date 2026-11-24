# GitLab

Merge requests, review threads, reviews and issues in the hub, through the `glab` CLI.

## Setup

Log in once with `glab auth login`. The lists are fetched outside any repo, so `glab` asks its
default host. If your GitLab is not gitlab.com, set it once, or the lists stay empty:
`glab config set --global host git.example.com`. Choose which GitLab sections show in
[configuration.md](configuration.md).

## Refreshing

The GitLab lists refresh every two minutes; `Ctrl+R` on a GitLab row or title, or a click on
the `↻` at the end of a title, refreshes that list now. A spinner takes the place of `↻` while
a list loads. Until the first fetch is done, each GitLab section shows its title with a
spinner. If a fetch fails, the title stays and says why, for example `⚠ glab is not installed`.

## Merge requests and their threads

**WORKSPACES** is always local: tmux sessions with their agents and
terminals. GitLab shows in its own sections. When a workspace's branch has a merge request, its
row shows `!140 ✓`: the MR and its pipeline (✓ passed, ✗ failed, ⟳ running). The MR's unresolved
review threads are listed under the MR in **MERGE REQUESTS** or **ASSIGNED MERGE REQUESTS**.
Select one to read it, `r` to reply in GitLab, `f` to put it in the Claude prompt of the
workspace that has the branch and jump there (edit it, then `Enter` sends), `Enter` to open it
in the browser. A workspace's MR refreshes every
minute in the background, every 10 seconds while a pipeline runs. `Ctrl+R` on a workspace fetches
it right away.

To close the loop: `f` sends a comment to Claude, `p` pushes the fix (and opens a draft MR the
first time), `v` resolves the thread with a "Fixed in <commit>" reply you can edit. The **Pipeline** tab (`4`) shows each stage and job
with its status and duration.

## Reviewing others' merge requests

MRs where you are a requested reviewer are listed under
**REVIEWS**, newest activity first, with author, pipeline and open threads. The panel shows the
description, threads, approvals and pipeline. `Enter` opens the MR in GitLab. `c` checks out its
code in a separate git worktree of your local clone (under `~/.cache/claude-hub/reviews/`),
opens it as a workspace and starts Claude with a review prompt typed in but not sent: adjust it,
then press `Enter`. `t` and `e` check it out the same way, with a shell or nvim instead of
Claude. Your own checkout is never touched. The review's workspace shows in **WORKSPACES**,
and the MR's row in **REVIEWS** names it (`▪ review-api-311`). `Enter` on the MR then goes to a
shell in the checkout (a new one if none is open), and `x` closes the workspace and deletes the
checkout. Reviews in progress come first, marked ✓ once you approve them. An approved MR without a
workspace moves to a folded **✓ APPROVED** group at the end; close the workspace with `x` to
send it there. If the author pushes and GitLab drops your approval, the MR moves back.

## Your merge requests

Open MRs you opened are listed under **MERGE REQUESTS**, and open MRs
assigned to you under **ASSIGNED MERGE REQUESTS**. An MR that is both yours and assigned to you
shows only under **MERGE REQUESTS**. Each row shows pipeline, open threads and approvals, and
the workspace that has its branch, if one is open. `Enter` goes to that workspace, and `c`, `t`
and `e` open Claude, a shell or nvim in it. Without a workspace, `Enter` opens the MR in GitLab,
and `c`, `t` and `e` first check out the MR's branch in a git worktree of your local clone (under
`~/.cache/claude-hub/mrs/`), so you can commit and push with `p`. Your own checkout is never
switched. For an MR from a fork, the clone needs a remote for the fork. `x` closes the workspace
and removes the worktree; git refuses while it has uncommitted changes, and commits stay on the
branch.

## Your issues

Open issues assigned to you are listed under **ISSUES**, newest activity
first, with labels. The panel shows the description. `Enter` opens the issue in GitLab. `c`
opens Claude in a new window of the workspace for your local clone, with a prompt that names
the issue typed in but not sent. The prompt asks Claude to make a branch first. The list
refreshes with the reviews.

## Links

Links in the panel (the `!140` line of an MR, "Open the thread in GitLab") are real terminal
hyperlinks: Ctrl+click (Cmd+click on macOS) opens them in the browser of the computer you sit
at, also over SSH.
