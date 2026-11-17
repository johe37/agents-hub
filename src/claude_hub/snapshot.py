import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

from .config import Config
from .git import GitCache, GitStatus, scan_roots
from .state import Session, load_sessions, worst_status
from .tmux import Pane, list_panes

SCAN_TTL = 30.0


@dataclass
class Repo:
    root: str
    status: GitStatus
    panes: list[Pane] = field(default_factory=list)
    sessions: list[Session] = field(default_factory=list)

    @property
    def name(self) -> str:
        return Path(self.root).name

    @property
    def badge(self) -> str | None:
        return worst_status(self.sessions)


@dataclass
class Snapshot:
    sessions: list[Session]
    repos: list[Repo]
    other_panes: list[Pane]
    session_by_pane: dict[str, Session]


class Collector:
    def __init__(self, cfg: Config) -> None:
        self.cfg = cfg
        self.git = GitCache()
        self.pool = ThreadPoolExecutor(max_workers=8)
        self._scanned: list[str] = []
        self._scanned_at = 0.0

    def _scan(self) -> list[str]:
        if time.monotonic() - self._scanned_at > SCAN_TTL:
            self._scanned = scan_roots(self.cfg.roots)
            self._scanned_at = time.monotonic()
        return self._scanned

    def collect(self) -> Snapshot:
        panes = list_panes()
        sessions = load_sessions({p.pane_id for p in panes}, self.cfg.stale_minutes)
        session_by_pane = {s.tmux_pane: s for s in sessions if s.tmux_pane and s.alive}

        pane_roots = dict(zip((p.pane_id for p in panes), self.pool.map(self.git.repo_root, [p.path for p in panes])))
        live = [s for s in sessions if s.alive and s.cwd]
        session_roots = dict(zip((s.session_id for s in live), self.pool.map(self.git.repo_root, [s.cwd for s in live])))

        roots = set(self._scan()) | {r for r in pane_roots.values() if r} | {r for r in session_roots.values() if r}
        statuses = dict(zip(roots, self.pool.map(self.git.status, roots)))
        repos = {r: Repo(r, statuses[r]) for r in roots}

        other = []
        for p in panes:
            root = pane_roots.get(p.pane_id)
            if root in repos:
                repos[root].panes.append(p)
            else:
                other.append(p)
        for s in live:
            root = session_roots.get(s.session_id)
            if root in repos:
                repos[root].sessions.append(s)
                s.repo_root = root

        ordered = sorted(repos.values(), key=lambda r: (not r.panes, r.name.lower()))
        return Snapshot(sessions, ordered, other, session_by_pane)
