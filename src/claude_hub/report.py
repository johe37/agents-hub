"""Hook handler. Runs on every hook event, so it must stay fast and stdlib-only."""

import json
import os
import subprocess
import sys
import time

from .config import STATE_DIR

STATUS_BY_EVENT = {
    "SessionStart": "idle",
    "UserPromptSubmit": "working",
    "PreToolUse": "working",
    "PostToolUse": "working",
    "Notification": "waiting",
    "Stop": "idle",
    "SessionEnd": "ended",
}


def _run(*args: str) -> str | None:
    try:
        out = subprocess.run(args, capture_output=True, text=True, timeout=2)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return out.stdout.strip() if out.returncode == 0 else None


def _git_info(cwd: str) -> dict:
    root = _run("git", "-C", cwd, "rev-parse", "--show-toplevel")
    if not root:
        return {"repo_root": None, "branch": None}
    # symbolic-ref also works on a branch without commits; detached HEAD falls back to the hash.
    branch = _run("git", "-C", cwd, "symbolic-ref", "--short", "-q", "HEAD") or _run(
        "git", "-C", cwd, "rev-parse", "--short", "HEAD"
    )
    return {"repo_root": root, "branch": branch}


def _tmux_info(pane: str | None) -> dict:
    if not pane:
        return {"tmux_pane": None, "tmux_session": None, "tmux_window": None}
    out = _run("tmux", "display-message", "-p", "-t", pane, "#{session_name}\t#{window_index}")
    session, _, window = (out or "\t").partition("\t")
    return {"tmux_pane": pane, "tmux_session": session or None, "tmux_window": window or None}


def report(data: dict) -> None:
    session_id = data.get("session_id")
    event = data.get("hook_event_name")
    if not session_id or event not in STATUS_BY_EVENT:
        return

    path = STATE_DIR / f"{session_id}.json"
    try:
        prev = json.loads(path.read_text())
    except (OSError, ValueError):
        prev = {}

    cwd = data.get("cwd") or prev.get("cwd") or os.getcwd()
    pane = os.environ.get("TMUX_PANE")

    state = dict(prev)
    state.update(
        session_id=session_id,
        cwd=cwd,
        status=STATUS_BY_EVENT[event],
        last_event=event,
        timestamp=time.time(),
        transcript_path=data.get("transcript_path") or prev.get("transcript_path"),
    )
    # git and tmux lookups cost a subprocess each; only redo them when the location changed.
    if prev.get("cwd") != cwd or "repo_root" not in prev or event == "Stop":
        state.update(_git_info(cwd))
    if prev.get("tmux_pane") != pane or "tmux_session" not in prev:
        state.update(_tmux_info(pane))
    if event == "Notification":
        state["message"] = data.get("message")
    elif event == "UserPromptSubmit":
        state["message"] = None

    STATE_DIR.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, indent=2))
    os.replace(tmp, path)


def main() -> int:
    # Never fail or print: hook stdout can end up in Claude's context.
    try:
        report(json.load(sys.stdin))
    except Exception:
        pass
    return 0
