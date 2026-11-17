import json
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from . import theme
from .config import STATE_DIR

STATUS_ORDER = {"waiting": 0, "working": 1, "idle": 2, "ended": 3, "stale": 4}
STATUS_STYLE = {
    "waiting": ("⏳", f"bold {theme.WARNING}"),
    "working": ("●", theme.INFO),
    "idle": ("○", theme.SUCCESS),
    "ended": ("✗", theme.DIM),
    "stale": ("?", theme.DIM),
}
PRUNE_AFTER = 24 * 3600


@dataclass
class Session:
    session_id: str
    cwd: str
    status: str
    last_event: str
    timestamp: float
    repo_root: str | None = None
    branch: str | None = None
    tmux_socket: str | None = None
    tmux_pane: str | None = None
    tmux_session: str | None = None
    tmux_window: str | None = None
    transcript_path: str | None = None
    message: str | None = None
    path: Path | None = None

    @property
    def name(self) -> str:
        return Path(self.repo_root or self.cwd).name

    @property
    def alive(self) -> bool:
        return self.status not in ("ended", "stale")


def load_sessions(live_panes: Callable[[str], set[str]], stale_minutes: float) -> list[Session]:
    """Read all status files and mark dead ones stale.

    live_panes(socket) returns the pane ids alive on that tmux server.
    """
    sessions = []
    now = time.time()
    for path in STATE_DIR.glob("*.json"):
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        fields = {k: data.get(k) for k in Session.__dataclass_fields__ if k != "path"}
        if not fields["session_id"]:
            continue
        fields["status"] = fields["status"] or "idle"
        fields["cwd"] = fields["cwd"] or ""
        fields["timestamp"] = fields["timestamp"] or 0.0
        sessions.append(Session(**fields, path=path))

    newest_in_pane: dict[tuple, Session] = {}
    for s in sessions:
        if s.tmux_pane and s.alive:
            key = (s.tmux_socket, s.tmux_pane)
            cur = newest_in_pane.get(key)
            if cur is None or s.timestamp > cur.timestamp:
                newest_in_pane[key] = s

    live_cache: dict[str, set[str]] = {}
    for s in sessions:
        if not s.alive:
            continue
        if s.tmux_pane and s.tmux_socket:
            if s.tmux_socket not in live_cache:
                live_cache[s.tmux_socket] = live_panes(s.tmux_socket)
            # A pane runs one Claude at a time, so older sessions in it are dead.
            alive_pane = s.tmux_pane in live_cache[s.tmux_socket]
            if not alive_pane or newest_in_pane[(s.tmux_socket, s.tmux_pane)] is not s:
                s.status = "stale"
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
