"""Screen previews of terminals. tmux pipes each pane's output to a file; pyte replays it into a screen.

capture-pane is not used: it crashes the tmux build on some machines (EL10's git snapshot).
"""

import os
import threading
from pathlib import Path

from .config import STATE_DIR

SCREENS_DIR = STATE_DIR / "screens"
MAX_LOG = 512 * 1024

try:
    import pyte
except ImportError:  # pragma: no cover - optional until installed
    pyte = None


def log_path(pane_id: str) -> Path:
    return SCREENS_DIR / f"{pane_id.lstrip('%')}.log"


class ScreenCache:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._screens: dict[str, tuple] = {}  # pane_id -> (screen, stream, offset)

    @property
    def available(self) -> bool:
        return pyte is not None

    def lines(self, pane_id: str, width: int, height: int) -> list[str] | None:
        """The pane's screen as text lines, or None when nothing is known yet."""
        if pyte is None:
            return None
        path = log_path(pane_id)
        with self._lock:
            entry = self._screens.get(pane_id)
            if entry is None:
                screen = pyte.Screen(width, height)
                stream = pyte.ByteStream(screen)
                entry = self._screens[pane_id] = [screen, stream, 0]
            screen, stream, offset = entry
            if (screen.columns, screen.lines) != (width, height):
                screen.resize(height, width)
            try:
                size = path.stat().st_size
            except OSError:
                return None
            if size < offset:
                offset = 0
            if size > offset:
                with open(path, "rb") as f:
                    f.seek(offset)
                    data = f.read(size - offset)
                stream.feed(data)
                offset = size
            if size > MAX_LOG:
                # The screen state lives in memory; the file only needs to hold what the next hub start replays.
                try:
                    os.truncate(path, 0)
                except OSError:
                    pass
                offset = 0
            entry[2] = offset
            lines = [line.rstrip() for line in screen.display]
            while lines and not lines[-1]:
                lines.pop()
            return lines

    def forget(self, live: set[str]) -> None:
        """Drop state and files of panes that no longer exist."""
        with self._lock:
            for pane_id in list(self._screens):
                if pane_id not in live:
                    del self._screens[pane_id]
        try:
            for f in SCREENS_DIR.glob("*.log"):
                if f"%{f.stem}" not in live:
                    f.unlink(missing_ok=True)
        except OSError:
            pass
