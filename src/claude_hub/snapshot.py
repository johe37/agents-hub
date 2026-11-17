import os
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

from . import theme
from .config import Config
from .git import GitCache, GitStatus, load_dir_cache, save_dir_cache, scan_dirs
from .state import STATUS_ORDER, Session, load_sessions, worst_status
from .tmux import SUMMARY_FILE, Pane, list_panes, live_panes, socket_path

@dataclass
class Window:
    session: str
    index: int
    name: str
    window_id: str
    command: str
    claude: Session | None = None


@dataclass
class Workspace:
    name: str
    path: str
    windows: list[Window] = field(default_factory=list)
    git: GitStatus | None = None

    @property
    def badge(self) -> str | None:
        return worst_status([w.claude for w in self.windows if w.claude])


@dataclass
class Snapshot:
    workspaces: list[Workspace]
    elsewhere: list[Session]
    recent_dirs: list[str]


def _windows(panes: list[Pane], claude_by_pane: dict[str, Session]) -> dict[str, list[Window]]:
    by_window: dict[str, list[Pane]] = {}
    for p in panes:
        by_window.setdefault(p.window_id, []).append(p)
    result: dict[str, list[Window]] = {}
    for window_panes in by_window.values():
        active = next((p for p in window_panes if p.active), window_panes[0])
        claude = next((claude_by_pane[p.pane_id] for p in window_panes if p.pane_id in claude_by_pane), None)
        w = Window(active.session, active.window_index, active.window_name, active.window_id, active.command, claude)
        result.setdefault(active.session, []).append(w)
    for windows in result.values():
        windows.sort(key=lambda w: w.index)
    return result


class Collector:
    def __init__(self, cfg: Config) -> None:
        self.cfg = cfg
        self.git = GitCache()
        self.pool = ThreadPoolExecutor(max_workers=8)
        self.hub_socket = socket_path()
        cached = load_dir_cache()
        self.dirs: list[tuple[str, bool]] = cached or []
        self._need_scan = cached is None

    def rescan(self) -> list[tuple[str, bool]]:
        self.dirs = scan_dirs(self.cfg.roots)
        save_dir_cache(self.dirs)
        return self.dirs

    def collect(self) -> Snapshot:
        panes = list_panes()
        hub_panes = {p.pane_id for p in panes}

        def live(socket: str) -> set[str]:
            return hub_panes if os.path.realpath(socket) == self.hub_socket else live_panes(socket)

        sessions = load_sessions(live, self.cfg.stale_minutes)
        in_hub = {
            s.tmux_pane: s
            for s in sessions
            if s.alive and s.tmux_pane and s.tmux_socket and os.path.realpath(s.tmux_socket) == self.hub_socket
        }
        elsewhere = [s for s in sessions if s.alive and s.tmux_pane not in in_hub]

        windows = _windows(panes, in_hub)
        workspaces = []
        for name, wins in windows.items():
            first = next(p for p in panes if p.session == name)
            workspaces.append(Workspace(name, first.workspace_path or first.session_path, wins))

        roots = list(self.pool.map(self.git.repo_root, [w.path for w in workspaces]))
        statuses = list(self.pool.map(lambda r: self.git.status(r) if r else None, roots))
        for ws, st in zip(workspaces, statuses):
            ws.git = st

        workspaces.sort(key=lambda w: (STATUS_ORDER.get(w.badge or "", 5), w.name.lower()))
        self._write_summary(sessions)

        if self._need_scan:
            self._need_scan = False
            self.rescan()
        recent = list(dict.fromkeys(s.cwd for s in sessions if s.cwd))
        return Snapshot(workspaces, elsewhere, recent)

    def _write_summary(self, sessions: list[Session]) -> None:
        """One-line summary shown in the tmux status bar of every workspace."""
        waiting = sum(1 for s in sessions if s.status == "waiting")
        text = (
            f"#[bg={theme.WARNING},fg={theme.BG},bold] ⏳ {waiting} waiting #[bg={theme.BAR},fg={theme.FG},nobold]"
            if waiting
            else ""
        )
        try:
            if not SUMMARY_FILE.exists() or SUMMARY_FILE.read_text() != text:
                SUMMARY_FILE.parent.mkdir(parents=True, exist_ok=True)
                SUMMARY_FILE.write_text(text)
        except OSError:
            pass


def shorten(path: str) -> str:
    home = str(Path.home())
    return "~" + path[len(home) :] if path == home or path.startswith(home + "/") else path
