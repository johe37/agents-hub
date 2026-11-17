import json
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from . import theme
from .config import STATE_DIR
from .transcript import tool_summary

STATUS_ORDER = {"waiting": 0, "working": 1, "idle": 2, "ended": 3, "stale": 4}
STATUS_STYLE = {
    "waiting": ("⏳", f"bold {theme.WARNING}"),
    "working": ("●", theme.INFO),
    "idle": ("○", theme.SUCCESS),
    "ended": ("✗", theme.DIM),
    "stale": ("?", theme.DIM),
    "unknown": ("·", theme.DIM),
}
PRUNE_AFTER = 24 * 3600
EVENTS_DIR = STATE_DIR / "events"


STATUS_BY_EVENT = {
    "SessionStart": "idle",
    "UserPromptSubmit": "working",
    "PreToolUse": "working",
    "PostToolUse": "working",
    "Notification": "waiting",
    "Stop": "idle",
    "SessionEnd": "ended",
}


@dataclass
class Session:
    session_id: str
    cwd: str
    status: str
    last_event: str
    timestamp: float
    tmux_socket: str | None = None
    tmux_pane: str | None = None
    tmux_session: str | None = None
    transcript_path: str | None = None
    message: str | None = None
    notification_type: str | None = None
    tool: str | None = None
    permission_mode: str | None = None
    repo_root: str | None = None
    path: Path | None = None

    @property
    def name(self) -> str:
        return Path(self.repo_root or self.cwd).name

    @property
    def alive(self) -> bool:
        return self.status not in ("ended", "stale")


def _read_event(path: Path) -> Session | None:
    """Turn the latest hook event of a session (written by hook.sh) into a Session."""
    try:
        record = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    event = record.get("event") or {}
    name = event.get("hook_event_name")
    if name not in STATUS_BY_EVENT or not event.get("session_id"):
        return None
    status = STATUS_BY_EVENT[name]
    if name == "Notification" and event.get("notification_type") == "agent_completed":
        # Sent to every open session when some other session finishes; it says nothing about this one.
        status = "idle"
    return Session(
        session_id=event["session_id"],
        cwd=event.get("cwd") or "",
        status=status,
        last_event=name,
        timestamp=float(record.get("time") or 0),
        tmux_socket=record.get("tmux") or None,
        tmux_pane=record.get("tmux_pane") or None,
        transcript_path=event.get("transcript_path"),
        message=event.get("message") if status == "waiting" else None,
        notification_type=event.get("notification_type") if status == "waiting" else None,
        tool=tool_summary(event.get("tool_name"), event.get("tool_input")) if name == "PreToolUse" else None,
        permission_mode=event.get("permission_mode"),
        path=path,
    )


def load_sessions(live_panes: Callable[[str], dict[str, str]], stale_minutes: float) -> list[Session]:
    """Read the latest event of every session and mark dead ones stale.

    live_panes(socket) returns {pane_id: tmux_session} for the panes alive on that tmux server.
    """
    now = time.time()
    sessions = [s for s in map(_read_event, EVENTS_DIR.glob("*.json")) if s is not None]

    newest_in_pane: dict[tuple, Session] = {}
    for s in sessions:
        if s.tmux_pane and s.alive:
            key = (s.tmux_socket, s.tmux_pane)
            cur = newest_in_pane.get(key)
            if cur is None or s.timestamp > cur.timestamp:
                newest_in_pane[key] = s

    live_cache: dict[str, dict[str, str]] = {}
    for s in sessions:
        if not s.alive:
            continue
        if s.tmux_pane and s.tmux_socket:
            if s.tmux_socket not in live_cache:
                live_cache[s.tmux_socket] = live_panes(s.tmux_socket)
            live = live_cache[s.tmux_socket]
            # A pane runs one Claude at a time, so older sessions in it are dead.
            if s.tmux_pane not in live or newest_in_pane[(s.tmux_socket, s.tmux_pane)] is not s:
                s.status = "stale"
            else:
                s.tmux_session = live[s.tmux_pane]
        elif now - s.timestamp > stale_minutes * 60:
            s.status = "stale"

    for s in sessions:
        if not s.alive and now - s.timestamp > PRUNE_AFTER and s.path:
            s.path.unlink(missing_ok=True)

    sessions.sort(key=lambda s: (STATUS_ORDER.get(s.status, 9), -s.timestamp))
    return sessions


def worst_status(sessions: list[Session]) -> str | None:
    alive = [s.status for s in sessions if s.alive]
    return min(alive, key=lambda st: STATUS_ORDER[st]) if alive else None


def age(ts: float) -> str:
    secs = max(0, int(time.time() - ts))
    for unit, size in (("d", 86400), ("h", 3600), ("m", 60)):
        if secs >= size:
            return f"{secs // size}{unit}"
    return f"{secs}s"
