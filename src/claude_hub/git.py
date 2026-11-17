import os
import subprocess
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

STATUS_TTL = 5.0
DIR_CACHE = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "claude-hub" / "dirs.tsv"


@dataclass
class GitStatus:
    branch: str | None = None
    dirty: int = 0
    ahead: int = 0
    behind: int = 0
    has_upstream: bool = False
    added: int = 0
    removed: int = 0
    files: list[tuple[str, str]] = field(default_factory=list)


def _git(cwd: str, *args: str) -> str | None:
    try:
        out = subprocess.run(["git", "-C", cwd, *args], capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return out.stdout if out.returncode == 0 else None


def scan_dirs(roots: list[Path]) -> list[tuple[str, bool]]:
    """Find directories under roots as (path, is_repo). Stops descending at a repo, which keeps the scan fast."""
    found: list[tuple[str, bool]] = []

    def walk(path: str, is_root: bool) -> None:
        try:
            entries = list(os.scandir(path))
        except OSError:
            return
        # .git is a directory in a normal repo and a file in worktrees and submodules.
        if any(e.name == ".git" for e in entries):
            found.append((path, True))
            return
        if not is_root:
            found.append((path, False))
        for e in sorted(entries, key=lambda e: e.name):
            if not e.name.startswith(".") and e.is_dir(follow_symlinks=False):
                walk(e.path, False)

    for root in roots:
        if root.is_dir():
            walk(str(root), True)
    return found


def load_dir_cache() -> list[tuple[str, bool]] | None:
    try:
        lines = DIR_CACHE.read_text().splitlines()
    except OSError:
        return None
    result = []
    for line in lines:
        kind, _, path = line.partition("\t")
        if path:
            result.append((path, kind == "repo"))
    return result


def save_dir_cache(dirs: list[tuple[str, bool]]) -> None:
    DIR_CACHE.parent.mkdir(parents=True, exist_ok=True)
    DIR_CACHE.write_text("".join(f"{'repo' if is_repo else 'dir'}\t{path}\n" for path, is_repo in dirs))


def _changed_file(line: str) -> tuple[str, str]:
    """(status, path) from a porcelain v2 line. Status is the two-letter XY code, or ?? for untracked."""
    kind = line[0]
    if kind == "?":
        return "??", line[2:]
    if kind == "2":
        return line.split(" ", 9)[1], line.split(" ", 9)[9].split("\t")[0]
    if kind == "u":
        return line.split(" ", 10)[1], line.split(" ", 10)[10]
    return line.split(" ", 8)[1], line.split(" ", 8)[8]


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
                    st.files.append(_changed_file(line))
        numstat = _git(repo, "diff", "--numstat", "HEAD")
        for line in (numstat or "").splitlines():
            added, removed, *_ = line.split("\t")
            if added.isdigit() and removed.isdigit():
                st.added += int(added)
                st.removed += int(removed)
        with self._lock:
            self._status[repo] = (now, st)
        return st
