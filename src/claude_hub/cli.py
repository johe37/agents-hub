import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

HOOK_EVENTS = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Notification", "Stop", "SessionEnd"]
TOOL_EVENTS = {"PreToolUse", "PostToolUse"}
SETTINGS_PATH = Path.home() / ".claude" / "settings.json"


def _is_ours(command: str) -> bool:
    return command.endswith("claude-hub report") or command.endswith("claude_hub/hook.sh")


def install_hooks(yes: bool) -> int:
    command = f"/bin/sh {Path(__file__).parent / 'hook.sh'}"
    settings = json.loads(SETTINGS_PATH.read_text()) if SETTINGS_PATH.exists() else {}
    hooks = settings.setdefault("hooks", {})

    changed = []
    for event in HOOK_EVENTS:
        groups = hooks.setdefault(event, [])
        if any(h.get("command") == command for g in groups for h in g.get("hooks", [])):
            continue
        # Replace older claude-hub hooks, such as the slower `claude-hub report`.
        for g in groups:
            g["hooks"] = [h for h in g.get("hooks", []) if not _is_ours(h.get("command", ""))]
        groups[:] = [g for g in groups if g["hooks"]]
        group: dict = {"hooks": [{"type": "command", "command": command}]}
        if event in TOOL_EVENTS:
            group = {"matcher": "*", **group}
        groups.append(group)
        changed.append(event)

    if not changed:
        print("Hooks already installed.")
        return 0
    print(f"Will set '{command}' as the claude-hub hook for these events in {SETTINGS_PATH}:")
    print("  " + ", ".join(changed))
    if not yes and input("Continue? [y/N] ").strip().lower() != "y":
        print("Aborted.")
        return 1
    if SETTINGS_PATH.exists():
        shutil.copy2(SETTINGS_PATH, SETTINGS_PATH.with_suffix(".json.bak"))
    SETTINGS_PATH.parent.mkdir(parents=True, exist_ok=True)
    SETTINGS_PATH.write_text(json.dumps(settings, indent=2) + "\n")
    print("Done. Backup saved as settings.json.bak. New Claude sessions will report status.")
    return 0


def _attach(tmux, config: str, session: str) -> None:
    if tmux.inside_hub_server():
        tmux.switch_client(session)
        return
    # Inside another tmux this nests a client; tmux refuses that unless $TMUX is unset.
    env = {k: v for k, v in os.environ.items() if k != "TMUX"}
    os.execvpe("tmux", ["tmux", "-L", tmux.SOCKET_NAME, "-f", config, "attach-session", "-t", f"={session}"], env)


def open_hub(cfg, target: str | None) -> int:
    """Start the engine if needed, then go to the hub, or straight to a workspace for a directory."""
    from . import tmux

    exe = os.path.abspath(sys.argv[0])
    config = tmux.write_config(exe, cfg.hub_key)
    if tmux.server_running():
        subprocess.run(["tmux", "-L", tmux.SOCKET_NAME, "source-file", config], capture_output=True)
    tmux.ensure_hub(exe, config)

    session = tmux.HUB_SESSION
    if target and os.path.isdir(target):
        session, err = tmux.ensure_workspace(os.path.realpath(target), "shell", None)
        if err:
            print(f"claude-hub: {err}", file=sys.stderr)
            return 1
    elif target:
        tmux.REQUEST_FILE.parent.mkdir(parents=True, exist_ok=True)
        tmux.REQUEST_FILE.write_text(target)
    _attach(tmux, config, session)
    return 0


def run_ui(cfg, mouse: bool) -> int:
    from .app import HubApp

    attach_to = HubApp(cfg).run(mouse=mouse)
    if attach_to:
        from . import tmux

        exe = os.path.abspath(sys.argv[0])
        config = tmux.write_config(exe, cfg.hub_key)
        env = {k: v for k, v in os.environ.items() if k != "TMUX"}
        os.execvpe("tmux", ["tmux", "-L", tmux.SOCKET_NAME, "-f", config, "attach-session", "-t", f"={attach_to}"], env)
    return 0


def main() -> int:
    commands = {"ui", "report", "install-hooks"}
    argv = sys.argv[1:]
    if argv and argv[0] not in commands and not argv[0].startswith("-"):
        from .config import load_config

        # `claude-hub <dir>` opens a workspace there; `claude-hub <query>` opens the picker.
        return open_hub(load_config(), " ".join(argv))

    parser = argparse.ArgumentParser(
        prog="claude-hub",
        usage="claude-hub [<dir> | <query>] | {ui,report,install-hooks}",
        description="Open the claude-hub workspace manager. Give a directory to open a workspace there, "
        "or a search query to open the picker with it.",
    )
    parser.add_argument("--no-mouse", action="store_true", help="disable mouse support")
    sub = parser.add_subparsers(dest="cmd")
    sub.add_parser("ui", help="run the hub UI in the current terminal, without the tmux engine")
    sub.add_parser("report", help="hook handler: read hook JSON from stdin")
    hooks = sub.add_parser("install-hooks", help="add claude-hub hooks to ~/.claude/settings.json")
    hooks.add_argument("-y", "--yes", action="store_true")
    args = parser.parse_args()

    if args.cmd == "report":
        from .report import main as report_main

        return report_main()
    if args.cmd == "install-hooks":
        return install_hooks(args.yes)

    from .config import load_config

    cfg = load_config()
    if args.cmd == "ui":
        return run_ui(cfg, mouse=cfg.mouse and not args.no_mouse)
    return open_hub(cfg, None)


if __name__ == "__main__":
    sys.exit(main())
