"""The hidden tmux engine: a private tmux server where every session is a workspace."""

import os
import re
import shlex
import subprocess
from dataclasses import dataclass
from pathlib import Path
from string import Template

from . import theme
from .config import STATE_DIR

SOCKET_NAME = "claude-hub"
HUB_SESSION = "_hub"
SUMMARY_FILE = STATE_DIR / "summary"
REQUEST_FILE = STATE_DIR / "request"

FIELDS = [
    "session_name",
    "@workspace_path",
    "session_path",
    "window_index",
    "window_name",
    "window_id",
    "pane_id",
    "pane_active",
    "pane_current_path",
    "pane_current_command",
]

# string.Template, because tmux formats are full of braces.
CONFIG_TEMPLATE = Template("""\
# Your own tmux config first (prefix, keys, theme); the hub settings below win where they overlap.
source-file -q ~/.tmux.conf
source-file -q ~/.config/tmux/tmux.conf
set -g default-terminal "tmux-256color"
set -as terminal-features ",*:RGB"
set -g escape-time 10
set -g mouse on
set -g history-limit 50000
set -g base-index 1
set -g renumber-windows on
set -g focus-events on
set -g status-interval 2
set -g status-position top
set -g status-justify left
set -g status-style "bg=$bar,fg=$fg"
set -g status-left-length 120
set -g status-left "#[range=user|hub,bg=$accent,fg=$bright,bold]  ◀  Claude Hub  #[norange,bg=$bar]  #[fg=$accent]◆ #[fg=$bright,bold]#S#[nobold,fg=$dim]#{?@workspace_label,  #{@workspace_label},}   "
set -g status-right-length 80
set -g status-right "#{?client_prefix,#[bg=$info fg=$bright bold] PREFIX #[bg=$bar nobold] ,}#(cat $summary 2>/dev/null)#[fg=$dim] %H:%M "
set -g window-status-separator ""
set -g window-status-format "#[fg=$muted]  #I #{?#{==:#W,claude},✻ ,}#W  "
set -g window-status-current-format "#[bg=$bg,fg=$accent]▎#[fg=$bright,bold]#I #{?#{==:#W,claude},✻ ,}#W  "
set -g message-style "bg=$accent,fg=$bright,bold"
set -g pane-border-status off
set -g pane-border-style "fg=$border"
set -g pane-active-border-style "fg=$accent"
set -g mode-style "bg=#264F78,fg=$bright"
bind -n MouseDown1Status if -F "#{==:#{mouse_status_range},hub}" "new-session -A -s $hub $hub_cmd" "select-window -t ="
set-hook -g client-session-changed[0] 'if -F "#{!=:#{session_name},$hub}" "display-message -d 3000 \\"$hint\\""'
set-hook -g client-attached[0] 'if -F "#{!=:#{session_name},$hub}" "display-message -d 3000 \\"$hint\\""'
""")


@dataclass
class Pane:
    session: str
    workspace_path: str
    session_path: str
    window_index: int
    window_name: str
    window_id: str
    pane_id: str
    active: bool
    path: str
    command: str


class NeedsAttach(Exception):
    """Raised outside the hub server: the caller should exit and attach to the target."""

    def __init__(self, session: str) -> None:
        super().__init__(session)
        self.session = session


def socket_path() -> str:
    base = os.environ.get("TMUX_TMPDIR", "/tmp")
    return os.path.realpath(os.path.join(base, f"tmux-{os.getuid()}", SOCKET_NAME))


def inside_hub_server() -> bool:
    current = os.environ.get("TMUX", "").split(",")[0]
    return bool(current) and os.path.realpath(current) == socket_path()


def _tmux(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["tmux", "-L", SOCKET_NAME, *args], capture_output=True, text=True)


def _error(out: subprocess.CompletedProcess) -> str | None:
    return (out.stderr.strip() or "tmux failed") if out.returncode != 0 else None


def write_config(exe: str) -> str:
    text = CONFIG_TEMPLATE.substitute(
        summary=shlex.quote(str(SUMMARY_FILE)),
        hub=HUB_SESSION,
        hub_cmd=shlex.quote(f"{exe} ui"),
        hint="Workspace #S  ·  click ◀ Claude Hub (top left) to go back",
        bar=theme.BAR,
        bg=theme.BG,
        fg=theme.FG,
        bright=theme.BRIGHT,
        muted=theme.MUTED,
        dim=theme.DIM,
        accent=theme.ACCENT,
        info=theme.INFO,
        border=theme.BORDER,
    )
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    path = STATE_DIR / "tmux.conf"
    path.write_text(text)
    return str(path)


def code_version() -> str:
    """Changes whenever the installed claude-hub code changes."""
    package = Path(__file__).parent
    return str(max(int(f.stat().st_mtime) for f in package.glob("*.py")))


def ensure_hub(exe: str, config: str) -> None:
    """Start the hub session, or restart its UI when the code changed since it started."""
    version = code_version()
    hub_cmd = f"{exe} ui"
    if HUB_SESSION not in list_sessions():
        subprocess.run(["tmux", "-L", SOCKET_NAME, "-f", config, "new-session", "-d", "-s", HUB_SESSION, hub_cmd])
    elif _tmux("show-options", "-gqv", "@hub_version").stdout.strip() != version:
        _tmux("respawn-pane", "-k", "-t", f"={HUB_SESSION}:", hub_cmd)
    _tmux("set-option", "-g", "@hub_version", version)


def server_running() -> bool:
    try:
        return _tmux("list-sessions").returncode == 0
    except OSError:
        return False


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
        p = line.split("\t")
        if len(p) != len(FIELDS) or p[0] == HUB_SESSION:
            continue
        panes.append(Pane(p[0], p[1], p[2], int(p[3]), p[4], p[5], p[6], p[7] == "1", p[8], p[9]))
    return panes


def live_panes(socket: str) -> set[str]:
    """Pane ids on any tmux server, given its socket path."""
    try:
        out = subprocess.run(["tmux", "-S", socket, "list-panes", "-a", "-F", "#{pane_id}"], capture_output=True, text=True)
    except OSError:
        return set()
    return set(out.stdout.split()) if out.returncode == 0 else set()


def list_sessions() -> list[str]:
    out = _tmux("list-sessions", "-F", "#{session_name}")
    return out.stdout.split() if out.returncode == 0 else []


def unique_session_name(name: str) -> str:
    # tmux rejects '.' and ':' in session names.
    base = re.sub(r"[.:\s]", "_", name) or "workspace"
    existing = set(list_sessions())
    candidate, n = base, 2
    while candidate in existing:
        candidate, n = f"{base}-{n}", n + 1
    return candidate


def switch_client(session: str) -> str | None:
    if not inside_hub_server():
        raise NeedsAttach(session)
    return _error(_tmux("switch-client", "-t", f"={session}"))


def jump_to_window(session: str, window_id: str) -> str | None:
    # Selecting first makes the window current in its session, so an attach lands there too.
    err = _error(_tmux("select-window", "-t", window_id))
    return err or switch_client(session)


def workspace_for_path(path: str) -> str | None:
    real = os.path.realpath(path)
    for p in list_panes():
        if os.path.realpath(p.workspace_path or p.session_path) == real:
            return p.session
    return None


def ensure_workspace(path: str, window: str, command: str | None) -> tuple[str, str | None]:
    """Return the workspace for path, creating it if needed. Returns (name, error)."""
    existing = workspace_for_path(path)
    if existing:
        return existing, None
    name = unique_session_name(Path(path).name)
    args = ["new-session", "-d", "-s", name, "-c", path, "-n", window]
    if command:
        args.append(command)
    err = _error(_tmux(*args))
    if err:
        return name, err
    home = str(Path.home())
    label = "~" + path[len(home) :] if path == home or path.startswith(home + "/") else path
    _tmux("set-option", "-t", f"={name}:", "@workspace_path", path)
    _tmux("set-option", "-t", f"={name}:", "@workspace_label", label)
    return name, None


def open_workspace(path: str, window: str, command: str | None) -> str | None:
    name, err = ensure_workspace(path, window, command)
    return err or switch_client(name)


def new_window(session: str, cwd: str, name: str, command: str | None) -> str | None:
    args = ["new-window", "-t", f"={session}:", "-c", cwd, "-n", name, "-P", "-F", "#{window_id}"]
    if command:
        args.append(command)
    out = _tmux(*args)
    if out.returncode != 0:
        return _error(out)
    return jump_to_window(session, out.stdout.strip())


def kill_session(session: str) -> str | None:
    return _error(_tmux("kill-session", "-t", f"={session}"))


def kill_window(window_id: str) -> str | None:
    return _error(_tmux("kill-window", "-t", window_id))


def rename_session(session: str, new: str) -> str | None:
    return _error(_tmux("rename-session", "-t", f"={session}", new))


def rename_window(window_id: str, new: str) -> str | None:
    return _error(_tmux("rename-window", "-t", window_id, new))


def detach() -> None:
    _tmux("detach-client")


def hide_hub_status() -> None:
    _tmux("set-option", "-t", f"={HUB_SESSION}:", "status", "off")
