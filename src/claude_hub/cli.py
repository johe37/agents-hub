import argparse
import json
import os
import shutil
import sys
from pathlib import Path

HOOK_EVENTS = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Notification", "Stop", "SessionEnd"]
TOOL_EVENTS = {"PreToolUse", "PostToolUse"}
SETTINGS_PATH = Path.home() / ".claude" / "settings.json"


def install_hooks(yes: bool) -> int:
    command = f"{os.path.abspath(sys.argv[0])} report"
    settings = json.loads(SETTINGS_PATH.read_text()) if SETTINGS_PATH.exists() else {}
    hooks = settings.setdefault("hooks", {})

    added = []
    for event in HOOK_EVENTS:
        groups = hooks.setdefault(event, [])
        if any(h.get("command") == command for g in groups for h in g.get("hooks", [])):
            continue
        group: dict = {"hooks": [{"type": "command", "command": command}]}
        if event in TOOL_EVENTS:
            group = {"matcher": "*", **group}
        groups.append(group)
        added.append(event)

    if not added:
        print("Hooks already installed.")
        return 0
    print(f"Will add '{command}' to these hooks in {SETTINGS_PATH}:")
    print("  " + ", ".join(added))
    if not yes and input("Continue? [y/N] ").strip().lower() != "y":
        print("Aborted.")
        return 1
    if SETTINGS_PATH.exists():
        shutil.copy2(SETTINGS_PATH, SETTINGS_PATH.with_suffix(".json.bak"))
    SETTINGS_PATH.parent.mkdir(parents=True, exist_ok=True)
    SETTINGS_PATH.write_text(json.dumps(settings, indent=2) + "\n")
    print("Done. Backup saved as settings.json.bak. New Claude sessions will report status.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(prog="claude-hub")
    parser.add_argument("--popup", action="store_true", help="exit after jumping to a pane")
    parser.add_argument("--no-mouse", action="store_true", help="disable mouse support")
    sub = parser.add_subparsers(dest="cmd")
    sub.add_parser("report", help="hook handler: read hook JSON from stdin")
    hooks = sub.add_parser("install-hooks", help="add claude-hub hooks to ~/.claude/settings.json")
    hooks.add_argument("-y", "--yes", action="store_true")
    args = parser.parse_args()

    if args.cmd == "report":
        from .report import main as report_main

        return report_main()
    if args.cmd == "install-hooks":
        return install_hooks(args.yes)

    from .app import HubApp
    from .config import load_config

    cfg = load_config()
    HubApp(cfg, popup=args.popup).run(mouse=cfg.mouse and not args.no_mouse)
    return 0


if __name__ == "__main__":
    sys.exit(main())
