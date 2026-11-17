import os
import re
import subprocess
from dataclasses import dataclass

FIELDS = [
    "session_name",
    "window_index",
    "window_name",
    "pane_id",
    "pane_current_path",
    "pane_current_command",
]


@dataclass
class Pane:
    session: str
    window_index: str
    window_name: str
    pane_id: str
    path: str
    command: str

    @property
    def location(self) -> str:
        return f"{self.session}:{self.window_index}"


def in_tmux() -> bool:
    return bool(os.environ.get("TMUX"))


def _tmux(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["tmux", *args], capture_output=True, text=True)


def list_panes() -> list[Pane]:
    fmt = "\t".join(f"#{{{f}}}" for f in FIELDS)
    try:
        out = _tmux("list-panes", "-a", "-F", fmt)
    except OSError:
        return []
    if out.returncode != 0:
        return []
    panes = []
    for line in out.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) == len(FIELDS):
            panes.append(Pane(*parts))
    return panes


def list_sessions() -> list[str]:
    out = _tmux("list-sessions", "-F", "#{session_name}")
    return out.stdout.split() if out.returncode == 0 else []


def session_name_for(name: str) -> str:
    # tmux rejects '.' and ':' in session names.
    return re.sub(r"[.:]", "_", name)


def jump_to_pane(pane_id: str) -> str | None:
    """Switch the current client to the pane. Returns an error message on failure."""
    if not in_tmux():
        return "Not running inside tmux"
    out = _tmux("display-message", "-p", "-t", pane_id, "#{session_name}\t#{window_id}")
    if out.returncode != 0:
        return f"Pane {pane_id} not found"
    session, window = out.stdout.strip().split("\t")
    _tmux("switch-client", "-t", session)
    _tmux("select-window", "-t", window)
    _tmux("select-pane", "-t", pane_id)
    return None


def ensure_session(name: str, cwd: str) -> str:
    name = session_name_for(name)
    if name not in list_sessions():
        _tmux("new-session", "-d", "-s", name, "-c", cwd)
    return name


def switch_to_session(name: str) -> str | None:
    if not in_tmux():
        return "Not running inside tmux"
    out = _tmux("switch-client", "-t", f"={name}")
    if out.returncode != 0:
        return out.stderr.strip()
    return None


def new_window(session: str, cwd: str, command: str | None = None, name: str | None = None) -> str | None:
    args = ["new-window", "-t", f"={session}:", "-c", cwd, "-P", "-F", "#{pane_id}"]
    if name:
        args += ["-n", name]
    if command:
        args.append(command)
    out = _tmux(*args)
    if out.returncode != 0:
        return out.stderr.strip()
    return jump_to_pane(out.stdout.strip())
