import json
import time
from dataclasses import dataclass
from pathlib import Path

from .config import STATE_DIR

STATUS_ORDER = {"waiting": 0, "working": 1, "idle": 2, "ended": 3, "stale": 4}
STATUS_STYLE = {
    "waiting": ("⏳", "bold yellow"),
    "working": ("●", "green"),
    "idle": ("○", "cyan"),
    "ended": ("✗", "dim"),
    "stale": ("?", "dim"),
}


@dataclass
class Session:
    session_id: str
    cwd: str
    status: str
    last_event: str
    timestamp: float
    repo_root: str | None = None
    branch: str | None = None
    tmux_pane: str | None = None
    tmux_session: str | None = None
    tmux_window: str | None = None
    transcript_path: str | None = None
    message: str | None = None
    path: Path | None = None

    @property
    def repo_name(self) -> str:
        return Path(self.repo_root or self.cwd).name

    @property
    def location(self) -> str:
        if not self.tmux_session:
            return "-"
        return f"{self.tmux_session}:{self.tmux_window}"

    @property
    def alive(self) -> bool:
        return self.status not in ("ended", "stale")


def load_sessions(live_panes: set[str] | None, stale_minutes: float) -> list[Session]:
    """Read all status files. live_panes=None means tmux is unavailable."""
    sessions = []
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

    now = time.time()
    newest_in_pane: dict[str, Session] = {}
    for s in sessions:
        if s.tmux_pane and s.alive:
            cur = newest_in_pane.get(s.tmux_pane)
            if cur is None or s.timestamp > cur.timestamp:
                newest_in_pane[s.tmux_pane] = s

    for s in sessions:
        if not s.alive:
            continue
        if s.tmux_pane and live_panes is not None:
            # A pane runs one Claude at a time, so older sessions in it are dead.
            if s.tmux_pane not in live_panes or newest_in_pane[s.tmux_pane] is not s:
                s.status = "stale"
        elif now - s.timestamp > stale_minutes * 60:
            s.status = "stale"

    sessions.sort(key=lambda s: (STATUS_ORDER.get(s.status, 9), -s.timestamp))
    return sessions


def worst_status(sessions: list[Session]) -> str | None:
    alive = [s.status for s in sessions if s.alive]
    return min(alive, key=lambda st: STATUS_ORDER[st]) if alive else None


def remove_dead(sessions: list[Session]) -> int:
    n = 0
    for s in sessions:
        if not s.alive and s.path:
            s.path.unlink(missing_ok=True)
            n += 1
    return n


def age(ts: float) -> str:
    secs = max(0, int(time.time() - ts))
    for unit, size in (("d", 86400), ("h", 3600), ("m", 60)):
        if secs >= size:
            return f"{secs // size}{unit}"
    return f"{secs}s"
