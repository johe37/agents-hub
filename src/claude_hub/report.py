"""Python fallback for hooks installed as `claude-hub report`. New installs use hook.sh."""

import json
import os
import sys
import time

from .config import STATE_DIR


def main() -> int:
    # Never fail or print: hook stdout can end up in Claude's context.
    try:
        event = json.load(sys.stdin)
        session_id = event.get("session_id")
        if not session_id:
            return 0
        events = STATE_DIR / "events"
        events.mkdir(parents=True, exist_ok=True)
        record = {
            "time": int(time.time()),
            "tmux": os.environ.get("TMUX", "").split(",")[0],
            "tmux_pane": os.environ.get("TMUX_PANE", ""),
            "event": event,
        }
        tmp = events / f".{session_id}.{os.getpid()}"
        tmp.write_text(json.dumps(record))
        os.replace(tmp, events / f"{session_id}.json")
    except Exception:
        pass
    return 0
