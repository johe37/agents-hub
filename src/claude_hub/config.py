import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

CONFIG_PATH = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / "claude-hub" / "config.toml"
STATE_DIR = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local" / "state")) / "claude-hub"


@dataclass
class Config:
    roots: list[Path] = field(default_factory=lambda: [Path.home() / "repos"])
    mouse: bool = True
    refresh_seconds: float = 2.0
    stale_minutes: float = 60.0


def load_config() -> Config:
    cfg = Config()
    if not CONFIG_PATH.exists():
        return cfg
    with CONFIG_PATH.open("rb") as f:
        data = tomllib.load(f)
    if "roots" in data:
        cfg.roots = [Path(r).expanduser() for r in data["roots"]]
    cfg.mouse = bool(data.get("mouse", cfg.mouse))
    cfg.refresh_seconds = float(data.get("refresh_seconds", cfg.refresh_seconds))
    cfg.stale_minutes = float(data.get("stale_minutes", cfg.stale_minutes))
    return cfg
