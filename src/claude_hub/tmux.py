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
POOL_SESSION = "_pool"
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
    "pane_pipe",
    "pane_width",
    "pane_height",
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
set -g status-left "#[range=user|hub,bg=$accent,fg=$bright,bold]  ◀  Claude Hub  #[nobold]$hub_key_label #[norange,bg=$bar]  #[fg=$accent]◆ #[fg=$bright,bold]#S#[nobold,fg=$dim]#{?@workspace_label,  #{@workspace_label},}   "
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
$hub_bind
# The session and window lists hide the pool of spare shells.
bind s choose-tree -Zs -f "#{!=:#{session_name},$pool}"
bind w choose-tree -Zw -f "#{!=:#{session_name},$pool}"
# Older versions showed a hint on every switch; unset it on servers that still run.
set-hook -gu client-session-changed[0]
set-hook -gu client-attached[0]
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
    piped: bool = False
    width: int = 80
    height: int = 24


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


def key_label(key: str) -> str:
    """Human name for a tmux key: M-h -> Alt+h, C-space -> Ctrl+Space."""
    label = key.replace("M-", "Alt+").replace("C-", "Ctrl+").replace("S-", "Shift+")
    return label.replace("space", "Space")


def write_config(exe: str, hub_key: str = "C-q") -> str:
    hub_cmd = shlex.quote(f"{exe} ui")
    text = CONFIG_TEMPLATE.substitute(
        summary=shlex.quote(str(SUMMARY_FILE)),
        hub=HUB_SESSION,
        pool=POOL_SESSION,
        hub_cmd=hub_cmd,
        hub_bind=f"bind -n {shlex.quote(hub_key)} new-session -A -s {HUB_SESSION} {hub_cmd}",
        hub_key_label=key_label(hub_key),
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


def list_all_panes() -> list[Pane]:
    """Every pane on the hub server, including the hidden _hub and _pool sessions."""
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
        if len(p) == len(FIELDS):
            panes.append(Pane(p[0], p[1], p[2], int(p[3]), p[4], p[5], p[6], p[7] == "1", p[8], p[9], p[10] == "1", int(p[11] or 80), int(p[12] or 24)))
    return panes


def list_panes() -> list[Pane]:
    """Panes in workspaces only."""
    return [p for p in list_all_panes() if not p.session.startswith("_")]


def spares(panes: list[Pane]) -> dict[str, str]:
    """Pre-started shells waiting in the pool, as {path: window_id}. A spare's window name is its path."""
    return {p.window_name: p.window_id for p in panes if p.session == POOL_SESSION}


def spawn_spare(path: str, pool_exists: bool) -> None:
    if pool_exists:
        _tmux("new-window", "-d", "-t", f"={POOL_SESSION}:", "-c", path, "-n", path)
    else:
        _tmux("new-session", "-d", "-s", POOL_SESSION, "-c", path, "-n", path)


def live_panes(socket: str) -> dict[str, str]:
    """Pane ids on any tmux server, given its socket path, as {pane_id: session_name}."""
    try:
        out = subprocess.run(
            ["tmux", "-S", socket, "list-panes", "-a", "-F", "#{pane_id}\t#{session_name}"],
            capture_output=True,
            text=True,
        )
    except OSError:
        return {}
    if out.returncode != 0:
        return {}
    return dict(line.split("\t", 1) for line in out.stdout.splitlines() if "\t" in line)


def list_sessions() -> list[str]:
    out = _tmux("list-sessions", "-F", "#{session_name}")
    return out.stdout.split() if out.returncode == 0 else []


def unique_session_name(name: str, taken: set[str] | None = None) -> str:
    # tmux rejects '.' and ':' in session names.
    base = re.sub(r"[.:\s]", "_", name).lstrip("_") or "workspace"
    taken = set(list_sessions()) if taken is None else taken
    candidate, n = base, 2
    while candidate in taken:
        candidate, n = f"{base}-{n}", n + 1
    return candidate


def _chain(*commands: list[str]) -> list[str]:
    args: list[str] = []
    for cmd in commands:
        if args:
            args.append(";")
        args.extend(cmd)
    return args


def run_and_switch(session: str, *commands: list[str]) -> str | None:
    """Run tmux commands and switch to session, all in one tmux call so it feels instant."""
    inside = inside_hub_server()
    if inside:
        commands = (*commands, ["switch-client", "-t", f"={session}"])
    if commands:
        err = _error(_tmux(*_chain(*commands)))
        if err:
            return err
    if not inside:
        raise NeedsAttach(session)
    return None


def switch_client(session: str) -> str | None:
    return run_and_switch(session)


def jump_to_window(session: str, window_id: str) -> str | None:
    # Selecting first makes the window current in its session, so an attach lands there too.
    return run_and_switch(session, ["select-window", "-t", window_id])


def _label(path: str) -> str:
    home = str(Path.home())
    return "~" + path[len(home) :] if path == home or path.startswith(home + "/") else path


def _create_commands(name: str, path: str, window: str, command: str | None, spare: str | None) -> list[list[str]]:
    options = [
        ["set-option", "-t", f"={name}:", "@workspace_path", path],
        ["set-option", "-t", f"={name}:", "@workspace_label", _label(path)],
    ]
    if spare and command is None:
        # A session always starts with a window, so start with a cheap placeholder and swap in the spare shell.
        return [
            ["new-session", "-d", "-s", name, "-c", path, "-n", "_placeholder", "cat"],
            ["move-window", "-s", spare, "-t", f"={name}:"],
            ["kill-window", "-t", f"={name}:_placeholder"],
            ["rename-window", "-t", spare, window],
            *options,
        ]
    return [["new-session", "-d", "-s", name, "-c", path, "-n", window, *([command] if command else [])], *options]


def create_workspace(name: str, path: str, window: str, command: str | None, spare: str | None = None) -> str | None:
    err = run_and_switch(name, *_create_commands(name, path, window, command, spare))
    if err and spare:
        return run_and_switch(name, *_create_commands(name, path, window, command, None))
    return err


def workspace_for_path(path: str) -> str | None:
    real = os.path.realpath(path)
    for p in list_panes():
        if os.path.realpath(p.workspace_path or p.session_path) == real:
            return p.session
    return None


def ensure_workspace(path: str, window: str, command: str | None) -> tuple[str, str | None]:
    """Return the workspace for path, creating it (without switching) if needed. Returns (name, error)."""
    existing = workspace_for_path(path)
    if existing:
        return existing, None
    name = unique_session_name(Path(path).name)
    return name, _error(_tmux(*_chain(*_create_commands(name, path, window, command, None))))


def new_window(session: str, cwd: str, name: str, command: str | None, spare: str | None = None) -> str | None:
    if spare and command is None:
        err = run_and_switch(
            session,
            ["move-window", "-s", spare, "-t", f"={session}:"],
            ["rename-window", "-t", spare, name],
            ["select-window", "-t", spare],
        )
        if not err:
            return None
    return run_and_switch(session, ["new-window", "-t", f"={session}:", "-c", cwd, "-n", name, *([command] if command else [])])


def pipe_pane(pane_id: str, path: str) -> str | None:
    """Stream the pane's output to a file, for the preview in the hub."""
    return _error(_tmux("pipe-pane", "-O", "-t", pane_id, f"cat >> {shlex.quote(path)}"))


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


def send_text(socket: str, pane: str, text: str) -> str | None:
    """Type text into a pane and press Enter, as if the user wrote it."""
    cmd = ["tmux", "-S", socket, "send-keys", "-t", pane, "-l", "--", text, ";", "send-keys", "-t", pane, "Enter"]
    try:
        return _error(subprocess.run(cmd, capture_output=True, text=True))
    except OSError as e:
        return str(e)


def send_keys(socket: str, pane: str, *keys: str) -> str | None:
    """Press named keys (Enter, Escape, ...) in a pane."""
    try:
        return _error(subprocess.run(["tmux", "-S", socket, "send-keys", "-t", pane, *keys], capture_output=True, text=True))
    except OSError as e:
        return str(e)


def hide_hub_status() -> None:
    _tmux("set-option", "-t", f"={HUB_SESSION}:", "status", "off")
