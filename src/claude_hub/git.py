import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path

STATUS_TTL = 5.0


@dataclass
class GitStatus:
    branch: str | None = None
    dirty: int = 0
    ahead: int = 0
    behind: int = 0
    has_upstream: bool = False


def _git(cwd: str, *args: str) -> str | None:
    try:
        out = subprocess.run(["git", "-C", cwd, *args], capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return out.stdout if out.returncode == 0 else None


def scan_roots(roots: list[Path], max_depth: int = 2) -> list[str]:
    found: list[str] = []

    def walk(path: Path, depth: int) -> None:
        if (path / ".git").exists():
            found.append(str(path))
            return
        if depth >= max_depth:
            return
        try:
            children = sorted(p for p in path.iterdir() if p.is_dir() and not p.name.startswith("."))
        except OSError:
            return
        for child in children:
            walk(child, depth + 1)

    for root in roots:
        if root.is_dir():
            walk(root, 0)
    return found


class GitCache:
    """Thread-safe caches for repo-root lookups and rate-limited git status."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._roots: dict[str, str | None] = {}
        self._status: dict[str, tuple[float, GitStatus]] = {}

    def repo_root(self, path: str) -> str | None:
        """Main repo root for a path. Worktrees resolve to the repo they belong to."""
        with self._lock:
            if path in self._roots:
                return self._roots[path]
        out = _git(path, "rev-parse", "--path-format=absolute", "--show-toplevel", "--git-common-dir")
        root = None
        if out:
            toplevel, common = out.splitlines()[:2]
            common_path = Path(common)
            root = str(common_path.parent) if common_path.name == ".git" else toplevel
        with self._lock:
            self._roots[path] = root
        return root

    def status(self, repo: str) -> GitStatus:
        now = time.monotonic()
        with self._lock:
            cached = self._status.get(repo)
        if cached and now - cached[0] < STATUS_TTL:
            return cached[1]
        st = GitStatus()
        out = _git(repo, "status", "--porcelain=v2", "--branch")
        if out is not None:
            for line in out.splitlines():
                if line.startswith("# branch.head "):
                    st.branch = line.split(" ", 2)[2]
                elif line.startswith("# branch.upstream "):
                    st.has_upstream = True
                elif line.startswith("# branch.ab "):
                    a, b = line.split()[2:4]
                    st.ahead, st.behind = int(a), -int(b)
                elif not line.startswith("#"):
                    st.dirty += 1
        with self._lock:
            self._status[repo] = (now, st)
        return st
