"""What a Claude Code session is about, read from its transcript (JSONL)."""

import json
import os
import threading
from dataclasses import dataclass

TEXT_LIMIT = 3000
INPUT_KEYS = ("command", "file_path", "pattern", "query", "url", "skill", "description", "prompt")


@dataclass
class Transcript:
    title: str | None = None
    first_prompt: str | None = None
    last_prompt: str | None = None
    last_text: str | None = None
    last_tool: str | None = None
    model: str | None = None
    turns: int = 0
    cost_usd: float | None = None
    lines_added: int = 0
    lines_removed: int = 0
    offset: int = 0


def tool_summary(name: str | None, tool_input) -> str | None:
    if not name:
        return None
    if isinstance(tool_input, dict):
        for key in INPUT_KEYS:
            value = tool_input.get(key)
            if isinstance(value, str) and value.strip():
                return f"{name}: {value.strip().splitlines()[0][:160]}"
    return name


def _prompt_text(record: dict) -> str | None:
    """The text of a user prompt typed by the user, or None for tool results and injected context."""
    if record.get("isMeta") or record.get("isSidechain"):
        return None
    content = (record.get("message") or {}).get("content")
    if isinstance(content, list):
        if any(isinstance(b, dict) and b.get("type") != "text" for b in content):
            return None
        content = "\n".join(b.get("text", "") for b in content if isinstance(b, dict))
    if not isinstance(content, str):
        return None
    text = " ".join(content.split())
    return text if text and not text.startswith("<") else None


def _apply(t: Transcript, record: dict) -> None:
    kind = record.get("type")
    if kind == "ai-title":
        t.title = record.get("aiTitle") or t.title
    elif kind == "user":
        text = _prompt_text(record)
        if text:
            t.first_prompt = t.first_prompt or text
            t.last_prompt = text
            t.turns += 1
    elif kind == "assistant":
        message = record.get("message") or {}
        t.model = message.get("model") or t.model
        for block in message.get("content") or []:
            if not isinstance(block, dict):
                continue
            if block.get("type") == "text" and block.get("text", "").strip():
                t.last_text = block["text"].strip()[-TEXT_LIMIT:]
            elif block.get("type") == "tool_use":
                t.last_tool = tool_summary(block.get("name"), block.get("input"))
    elif kind == "cost-state":
        t.cost_usd = record.get("totalCostUSD")
        t.lines_added = record.get("totalLinesAdded") or 0
        t.lines_removed = record.get("totalLinesRemoved") or 0


class TranscriptCache:
    """Parses each transcript once, then only the bytes appended since the last look."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._items: dict[str, Transcript] = {}

    def get(self, path: str | None) -> Transcript | None:
        if not path:
            return None
        try:
            size = os.path.getsize(path)
        except OSError:
            return None
        with self._lock:
            t = self._items.get(path)
            if t is None or t.offset > size:
                t = self._items[path] = Transcript()
        if t.offset == size:
            return t
        try:
            with open(path, "rb") as f:
                f.seek(t.offset)
                data = f.read(size - t.offset)
        except OSError:
            return t
        # A line still being written has no newline yet; it is parsed on the next refresh.
        complete = data.rfind(b"\n") + 1
        for line in data[:complete].splitlines():
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if isinstance(record, dict):
                _apply(t, record)
        t.offset += complete
        return t
