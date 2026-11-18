import os
import threading
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

from . import theme
from .config import Config
from .git import GitCache, GitStatus, load_dir_cache, save_dir_cache, scan_dirs
from .state import STATUS_ORDER, Session, load_sessions, worst_status
from .screens import SCREENS_DIR, ScreenCache, log_path
from .tmux import POOL_SESSION, SUMMARY_FILE, Pane, kill_window, list_all_panes, live_panes, pipe_pane, socket_path, spares, spawn_spare
from .transcript import Transcript, TranscriptCache

@dataclass
class Window:
    session: str
    index: int
    name: str
    window_id: str
    pane_id: str
    command: str
    claude: Session | None = None
    width: int = 80
    height: int = 24
    piped: bool = False
    screen: list[str] | None = None


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
class Agent:
    """One Claude session: in a hub workspace, outside the hub, or a claude window without hook data."""

    key: str
    status: str
    session: Session | None = None
    workspace: Workspace | None = None
    window: Window | None = None
    transcript: Transcript | None = None
    git: GitStatus | None = None

    @property
    def title(self) -> str:
        t = self.transcript
        if t and (t.title or t.first_prompt):
            return t.title or t.first_prompt
        if self.window and self.window.name != "claude":
            return self.window.name
        return "Claude"

    @property
    def name(self) -> str:
        if self.workspace:
            return self.workspace.name
        return self.session.name if self.session else "?"

    @property
    def path(self) -> str:
        if self.workspace:
            return self.workspace.path
        return self.session.cwd if self.session else ""

    @property
    def timestamp(self) -> float:
        return self.session.timestamp if self.session else 0.0

    @property
    def in_hub(self) -> bool:
        return self.workspace is not None

    @property
    def target(self) -> tuple[str, str] | None:
        """(tmux socket, pane id) where the agent reads its input, if known."""
        if self.workspace and self.window:
            return socket_path(), self.window.pane_id
        s = self.session
        if s and s.tmux_socket and s.tmux_pane:
            return s.tmux_socket, s.tmux_pane
        return None


@dataclass
class Snapshot:
    workspaces: list[Workspace]
    agents: list[Agent]
    recent_dirs: list[str]
    spares: dict[str, str] = field(default_factory=dict)

    @property
    def idle_workspaces(self) -> list[Workspace]:
        busy = {a.workspace.name for a in self.agents if a.workspace}
        return [ws for ws in self.workspaces if ws.name not in busy]


def _windows(panes: list[Pane], claude_by_pane: dict[str, Session]) -> dict[str, list[Window]]:
    by_window: dict[str, list[Pane]] = {}
    for p in panes:
        by_window.setdefault(p.window_id, []).append(p)
    result: dict[str, list[Window]] = {}
    for window_panes in by_window.values():
        active = next((p for p in window_panes if p.active), window_panes[0])
        claude = next((claude_by_pane[p.pane_id] for p in window_panes if p.pane_id in claude_by_pane), None)
        w = Window(
            active.session, active.window_index, active.window_name, active.window_id, active.pane_id, active.command, claude,
            active.width, active.height, active.piped,
        )
        result.setdefault(active.session, []).append(w)
    for windows in result.values():
        windows.sort(key=lambda w: w.index)
    return result


class Collector:
    def __init__(self, cfg: Config) -> None:
        self.cfg = cfg
        self.git = GitCache()
        self.transcripts = TranscriptCache()
        self.screens = ScreenCache()
        self.pool = ThreadPoolExecutor(max_workers=8)
        self.hub_socket = socket_path()
        # Path the picker currently points at; the pool keeps a shell ready there too.
        self.speculative: str | None = None
        self._pool_lock = threading.Lock()
        cached = load_dir_cache()
        self.dirs: list[tuple[str, bool]] = cached or []
        self._need_scan = cached is None

    def rescan(self) -> list[tuple[str, bool]]:
        self.dirs = scan_dirs(self.cfg.roots)
        save_dir_cache(self.dirs)
        return self.dirs

    def collect(self) -> Snapshot:
        all_panes = list_all_panes()
        panes = [p for p in all_panes if not p.session.startswith("_")]
        hub_panes = {p.pane_id: p.session for p in all_panes}

        def live(socket: str) -> dict[str, str]:
            return hub_panes if os.path.realpath(socket) == self.hub_socket else live_panes(socket)

        sessions = load_sessions(live, self.cfg.stale_minutes)
        in_hub = {
            s.tmux_pane: s
            for s in sessions
            if s.alive and s.tmux_pane and s.tmux_socket and os.path.realpath(s.tmux_socket) == self.hub_socket
        }
        elsewhere = [s for s in sessions if s.alive and s.tmux_pane not in in_hub]
        for s in elsewhere:
            s.repo_root = self.git.repo_root(s.cwd) if s.cwd else None

        windows = _windows(panes, in_hub)
        workspaces = []
        for name, wins in windows.items():
            first = next(p for p in panes if p.session == name)
            workspaces.append(Workspace(name, first.workspace_path or first.session_path, wins))
        self._previews([w for ws in workspaces for w in ws.windows if not w.claude and w.command != "claude"], all_panes)

        roots = list(self.pool.map(self.git.repo_root, [w.path for w in workspaces]))
        statuses = list(self.pool.map(lambda r: self.git.status(r) if r else None, roots))
        for ws, st in zip(workspaces, statuses):
            ws.git = st

        workspaces.sort(key=lambda w: w.name.lower())
        agents = self._agents(workspaces, elsewhere)
        self._write_summary(sessions)

        if self._need_scan:
            self._need_scan = False
            self.rescan()
        recent = list(dict.fromkeys(s.cwd for s in sessions if s.cwd))
        if self.cfg.prewarm:
            self._fill_pool({w.path for w in workspaces})
        return Snapshot(workspaces, agents, recent, spares(list_all_panes()))

    def _agents(self, workspaces: list[Workspace], elsewhere: list[Session]) -> list[Agent]:
        agents: list[Agent] = []
        for ws in workspaces:
            for w in ws.windows:
                if w.claude:
                    agents.append(Agent(f"agent:{w.claude.session_id}", w.claude.status, w.claude, ws, w, git=ws.git))
                elif w.command == "claude":
                    agents.append(Agent(f"window:{w.window_id}", "unknown", None, ws, w, git=ws.git))
        roots = list(self.pool.map(lambda s: self.git.repo_root(s.cwd) if s.cwd else None, elsewhere))
        statuses = list(self.pool.map(lambda r: self.git.status(r) if r else None, roots))
        for s, st in zip(elsewhere, statuses):
            agents.append(Agent(f"agent:{s.session_id}", s.status, s, git=st))
        transcripts = list(self.pool.map(lambda a: self.transcripts.get(a.session.transcript_path) if a.session else None, agents))
        for a, t in zip(agents, transcripts):
            a.transcript = t
        agents.sort(key=lambda a: (STATUS_ORDER.get(a.status, 3), -a.timestamp, a.title.lower()))
        return agents

    def _previews(self, terminals: list[Window], all_panes: list[Pane]) -> None:
        """Keep every terminal pane piped to a log and replay the logs into screen previews."""
        if not self.screens.available:
            return
        SCREENS_DIR.mkdir(parents=True, exist_ok=True)
        for w in terminals:
            if not w.piped:
                pipe_pane(w.pane_id, str(log_path(w.pane_id)))
                w.piped = True
            w.screen = self.screens.lines(w.pane_id, w.width, w.height)
        self.screens.forget({p.pane_id for p in all_panes})

    def _fill_pool(self, paths: set[str]) -> None:
        """Keep one started shell ready per workspace, so a new terminal shows its prompt at once."""
        with self._pool_lock:
            wanted = paths | ({self.speculative} if self.speculative else set())
            seen: set[str] = set()
            for p in list_all_panes():
                if p.session != POOL_SESSION:
                    continue
                if p.window_name in seen or p.window_name not in wanted:
                    kill_window(p.window_id)
                seen.add(p.window_name)
            pool_exists = bool(seen)
            # A few per round, so a hub start with many workspaces does not spawn a burst of shells.
            for path in sorted(wanted - seen)[:3]:
                if os.path.isdir(path):
                    spawn_spare(path, pool_exists)
                    pool_exists = True

    def prewarm(self, path: str | None) -> None:
        self.speculative = path
        if not path or not os.path.isdir(path):
            return
        with self._pool_lock:
            current = spares(list_all_panes())
            if path not in current:
                spawn_spare(path, bool(current))

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
