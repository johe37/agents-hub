import os
from pathlib import Path

from rich.text import Text
from textual import events, on, work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.screen import ModalScreen
from textual.suggester import Suggester
from textual.widgets import Button, Footer, Input, Label, OptionList, Static, Tree
from textual.widgets.option_list import Option

from . import fuzzy, tmux
from . import theme as T
from .config import Config
from .snapshot import Collector, Snapshot, Window, Workspace, shorten
from .state import STATUS_STYLE, Session, age

WINDOW_KINDS = {"claude": "claude", "shell": None, "nvim": "nvim"}

HELP = """\
[b]claude-hub[/b]

[b]Workspaces[/b]
  Enter           jump to the workspace or window
  n               open a workspace (fuzzy search or type a path)
  c / t / e       new claude / shell / nvim window
  x               close window or workspace
  R               rename
  Space / l / h   expand / collapse
  j k / arrows    move
  /               filter, Esc clears
  q               leave (everything keeps running)

[b]From any workspace[/b]
  Click ◀ Claude Hub in the top bar, or use your tmux prefix + s
  and pick the _hub session.

[b]Mouse[/b]
  Click selects, click again (or double-click) opens.
  Click ▸/▾ to expand. Click footer keys.
"""


def status_text(status: str | None, with_name: bool = True) -> Text:
    if status is None:
        return Text("")
    symbol, style = STATUS_STYLE.get(status, ("·", ""))
    return Text(f"{symbol} {status}" if with_name else symbol, style=style)


class HelpScreen(ModalScreen):
    DEFAULT_CSS = """
    HelpScreen { align: center middle; }
    HelpScreen > Static { width: auto; padding: 1 2; border: round #3C3C3C; background: $panel; }
    """

    def compose(self) -> ComposeResult:
        yield Static(HELP)

    def on_key(self, event: events.Key) -> None:
        event.stop()
        self.dismiss()

    def on_click(self) -> None:
        self.dismiss()


class ConfirmScreen(ModalScreen[bool]):
    DEFAULT_CSS = """
    ConfirmScreen { align: center middle; }
    ConfirmScreen > Vertical { width: 64; height: auto; padding: 1 2; border: round #3C3C3C; background: $panel; }
    ConfirmScreen .title { color: $warning; text-style: bold; margin-bottom: 1; }
    ConfirmScreen Horizontal { height: auto; margin-top: 1; align-horizontal: right; }
    ConfirmScreen Button { margin-left: 2; min-width: 12; background: #313131; color: #CCCCCC; text-style: none; }
    ConfirmScreen Button:hover { background: #3C3C3C; }
    ConfirmScreen #yes:focus { background: #F85149; color: #FFFFFF; text-style: bold; }
    ConfirmScreen #no:focus { background: #0078D4; color: #FFFFFF; text-style: bold; }
    """
    BINDINGS = [
        Binding("y", "answer(True)", "Yes"),
        Binding("n,escape", "answer(False)", "No"),
        Binding("left,h", "app.focus_previous", show=False),
        Binding("right,l", "app.focus_next", show=False),
    ]

    def __init__(self, question: str) -> None:
        super().__init__()
        self.question = question

    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label("⚠  Are you sure?", classes="title")
            yield Label(self.question)
            with Horizontal():
                yield Button(" Yes  y ", id="yes", compact=True)
                yield Button(" No  n ", id="no", compact=True)

    def on_mount(self) -> None:
        # Start on No, so a stray Enter never closes anything.
        self.query_one("#no", Button).focus()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.dismiss(event.button.id == "yes")

    def action_answer(self, value: bool) -> None:
        self.dismiss(value)


class RenameScreen(ModalScreen[str | None]):
    DEFAULT_CSS = """
    RenameScreen { align: center middle; }
    RenameScreen > Vertical { width: 60; height: auto; padding: 1 2; border: round #3C3C3C; background: $panel; }
    """
    BINDINGS = [Binding("escape", "cancel", "Cancel")]

    def __init__(self, title: str, value: str) -> None:
        super().__init__()
        self.title_text = title
        self.value = value

    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label(self.title_text)
            yield Input(value=self.value)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        self.dismiss(event.value.strip() or None)

    def action_cancel(self) -> None:
        self.dismiss(None)


PATH_PREFIXES = ("/", "~", ".")


def _split_path(text: str) -> tuple[Path, str]:
    """Split typed text into the directory to list and the fragment typed after it."""
    if text in ("~", ".", "..") or text.endswith("/"):
        return Path(text).expanduser(), ""
    parent, _, fragment = text.rpartition("/")
    return Path(parent or "/").expanduser() if parent or text.startswith("/") else Path("."), fragment


def _subdirs(parent: Path, fragment: str) -> list[str]:
    try:
        entries = [e for e in os.scandir(parent) if e.is_dir()]
    except OSError:
        return []
    show_hidden = fragment.startswith(".")
    return sorted(e.name for e in entries if show_hidden or not e.name.startswith("."))


class PathSuggester(Suggester):
    """Inline completion for typed paths, like a shell. Accept it with the right arrow."""

    def __init__(self) -> None:
        super().__init__(use_cache=False, case_sensitive=True)

    async def get_suggestion(self, value: str) -> str | None:
        if not value.startswith(PATH_PREFIXES) or value in ("~", "."):
            return None
        parent, fragment = _split_path(value)
        if not fragment:
            return None
        for name in _subdirs(parent, fragment):
            if name.startswith(fragment) and name != fragment:
                return value + name[len(fragment) :] + "/"
        return None


class DirPicker(ModalScreen[tuple[str, str] | None]):
    """Pick a directory for a workspace. Returns (path, window kind)."""

    DEFAULT_CSS = """
    DirPicker { align: center middle; }
    DirPicker > Vertical { width: 90%; height: 85%; padding: 0 1; border: round #3C3C3C; background: $panel; }
    DirPicker OptionList { background: $panel; }
    DirPicker OptionList { height: 1fr; border: none; padding: 0; }
    DirPicker OptionList > .option-list--option-highlighted { background: #04395E; color: #FFFFFF; text-style: none; }
    DirPicker .title { color: #FFFFFF; text-style: bold; padding: 1 0 0 0; }
    DirPicker .hint { color: $text-muted; height: 1; }
    """
    BINDINGS = [
        Binding("escape", "cancel", "Cancel"),
        Binding("down,ctrl+n,ctrl+j", "move(1)", show=False, priority=True),
        Binding("up,ctrl+p,ctrl+k", "move(-1)", show=False, priority=True),
        Binding("tab", "complete", "Complete", priority=True),
        Binding("ctrl+o", "choose('claude')", "With Claude", priority=True),
        Binding("ctrl+r", "rescan", "Rescan", priority=True),
    ]

    def __init__(self, candidates: list[tuple[str, str, str]], rescan, query: str = "") -> None:
        super().__init__()
        self.candidates = candidates
        self.rescan_fn = rescan
        self.initial_query = query
        self._prewarm_timer = None

    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label("Open workspace", classes="title")
            yield Input(
                value=self.initial_query,
                placeholder="Search repos and directories, or type a path like ~/repos/…",
                suggester=PathSuggester(),
            )
            yield OptionList()
            yield Label(self._hint(), classes="hint")

    @staticmethod
    def _hint() -> Text:
        t = Text()
        for key, what in (("↵", "open with terminal"), ("^O", "open with Claude"), ("⇥", "complete"),
                          ("^R", "rescan"), ("esc", "close")):
            t.append(f" {key} ", style=f"{T.FG} on {T.INPUT}")
            t.append(f" {what}   ", style=T.MUTED)
        return t

    def on_mount(self) -> None:
        self._update(self.initial_query)

    def _path_options(self, text: str) -> list[Option]:
        parent, fragment = _split_path(text)
        items: list[Option] = []
        typed = Path(text).expanduser()
        if typed.is_dir():
            real = str(typed.resolve())
            items.append(Option(Text(f"Open {shorten(real)}", style="bold"), id=real))
        names = fuzzy.rank(fragment, _subdirs(parent, fragment), key=lambda n: n)
        open_paths = {c[0] for c in self.candidates if c[2] == "open"}
        for name in names[:300]:
            full = str((parent / name).resolve())
            if any(o.id == full for o in items):
                continue
            prompt = Text(shorten(full) + "/")
            if full in open_paths:
                prompt.append("  ● open", style=T.SUCCESS)
            elif os.path.exists(os.path.join(full, ".git")):
                prompt.append("  repo", style=T.PURPLE)
            items.append(Option(prompt, id=full))
        return items

    def _search_options(self, text: str) -> list[Option]:
        items: list[Option] = []
        for path, label, tag in fuzzy.rank(text, self.candidates, key=lambda c: c[1])[:300]:
            prompt = Text(label + ("/" if tag == "dir" else ""))
            if tag == "open":
                prompt.append("  ● open", style=T.SUCCESS)
            elif tag in ("recent", "dir"):
                prompt.append(f"  {tag}", style=T.DIM)
            items.append(Option(prompt, id=path))
        return items

    def _update(self, text: str) -> None:
        items = self._path_options(text) if text.startswith(PATH_PREFIXES) else self._search_options(text)
        options = self.query_one(OptionList)
        options.clear_options()
        options.add_options(items)
        if items:
            options.highlighted = 0

    @on(Input.Changed)
    def on_changed(self, event: Input.Changed) -> None:
        self._update(event.value.strip())

    def action_move(self, delta: int) -> None:
        options = self.query_one(OptionList)
        if options.option_count:
            current = options.highlighted or 0
            options.highlighted = max(0, min(options.option_count - 1, current + delta))

    def action_complete(self) -> None:
        """Fill the input with the highlighted directory, so the list shows what is inside it."""
        options = self.query_one(OptionList)
        if options.highlighted is None:
            return
        path = options.get_option_at_index(options.highlighted).id
        box = self.query_one(Input)
        box.value = shorten(path).rstrip("/") + "/"
        box.cursor_position = len(box.value)

    @on(OptionList.OptionHighlighted)
    def on_highlighted(self, event: OptionList.OptionHighlighted) -> None:
        # Start a shell for the highlighted directory while the user decides, so Enter opens it at once.
        if self._prewarm_timer is not None:
            self._prewarm_timer.stop()
        path = event.option.id
        self._prewarm_timer = self.set_timer(0.15, lambda: self.app.prewarm(path))

    def action_choose(self, kind: str) -> None:
        options = self.query_one(OptionList)
        if options.highlighted is None:
            return
        self.dismiss((options.get_option_at_index(options.highlighted).id, kind))

    @work(thread=True, exclusive=True)
    def action_rescan(self) -> None:
        self.app.call_from_thread(self.notify, "Scanning for repos…")
        candidates = self.rescan_fn()
        self.app.call_from_thread(self._set_candidates, candidates)

    def _set_candidates(self, candidates: list[tuple[str, str, str]]) -> None:
        self.candidates = candidates
        self._update(self.query_one(Input).value.strip())
        self.notify(f"Found {sum(1 for c in candidates if c[2] == 'repo')} repos")

    @on(Input.Submitted)
    def on_submitted(self) -> None:
        self.action_choose("shell")

    @on(OptionList.OptionSelected)
    def on_selected(self, event: OptionList.OptionSelected) -> None:
        self.dismiss((event.option.id, "shell"))

    def action_cancel(self) -> None:
        self.dismiss(None)


class WorkspaceTree(Tree):
    ICON_NODE = "▸ "
    ICON_NODE_EXPANDED = "▾ "
    BINDINGS = [
        Binding("enter", "select_cursor", "Open"),
        Binding("n", "app.new_workspace", "New"),
        Binding("c", "app.new_window('claude')", "Claude"),
        Binding("t", "app.new_window('shell')", "Shell"),
        Binding("e", "app.new_window('nvim')", "nvim"),
        Binding("x", "app.close", "Close"),
        Binding("R", "app.rename", "Rename"),
        Binding("space", "toggle_node", "Expand", show=False),
        Binding("j", "cursor_down", show=False),
        Binding("k", "cursor_up", show=False),
        Binding("l", "expand", show=False),
        Binding("h", "collapse", show=False),
    ]

    async def _on_click(self, event: events.Click) -> None:
        # Click selects, click on the selected row opens it.
        event.prevent_default()
        meta = event.style.meta
        if "line" not in meta:
            return
        line = meta["line"]
        if meta.get("toggle"):
            node = self.get_node_at_line(line)
            if node is not None:
                node.toggle()
        elif line == self.cursor_line:
            self.action_select_cursor()
        else:
            self.cursor_line = line

    def action_expand(self) -> None:
        node = self.cursor_node
        if node is not None and node.allow_expand:
            node.expand()

    def action_collapse(self) -> None:
        node = self.cursor_node
        if node is None:
            return
        if node.is_expanded:
            node.collapse()
        elif node.parent is not None and node.parent is not self.root:
            self.move_cursor(node.parent)
            node.parent.collapse()


class HubApp(App):
    TITLE = "claude-hub"
    ENABLE_COMMAND_PALETTE = False
    CSS = """
    #header { height: 2; background: $surface; border-bottom: solid #2B2B2B; }
    #header-left { width: 1fr; }
    #header-right { width: auto; }
    WorkspaceTree { height: 1fr; padding: 0 1; background: $background; }
    WorkspaceTree > .tree--guides { color: #404040; }
    WorkspaceTree > .tree--guides-hover { color: #404040; }
    WorkspaceTree > .tree--guides-selected { color: #0078D4; }
    Footer { border-top: none; }
    Input { background: #313131; border: tall #3C3C3C; }
    Input:focus { border: tall #0078D4; }
    #filter { display: none; }
    #filter.visible { display: block; }
    #empty { display: none; padding: 1 2; color: $text-muted; }
    #empty.visible { display: block; }
    """
    BINDINGS = [
        Binding("q", "leave", "Leave"),
        Binding("slash", "filter", "Filter"),
        Binding("question_mark", "help", "Help"),
        Binding("escape", "clear_filter", show=False),
    ]

    def __init__(self, cfg: Config) -> None:
        super().__init__()
        self.cfg = cfg
        self.collector = Collector(cfg)
        self.snapshot: Snapshot | None = None
        self.filter_text = ""
        self.collapsed: set[tuple] = set()
        self._lines: list[tuple] = []
        self._signature: object = None

    def compose(self) -> ComposeResult:
        title = Text()
        title.append(" ◆ ", style=f"bold {T.ACCENT}")
        title.append("Claude Hub", style=f"bold {T.BRIGHT}")
        title.append("   click ◀ Claude Hub in any workspace to come back", style=T.MUTED)
        with Horizontal(id="header"):
            yield Static(title, id="header-left")
            yield Static("", id="header-right")
        yield Static("No workspaces yet. Press [b]n[/b] to create one.", id="empty")
        yield WorkspaceTree("workspaces")
        yield Input(placeholder="filter…", id="filter")
        yield Footer()

    def on_mount(self) -> None:
        self.register_theme(T.textual_theme())
        self.theme = "vscode-dark-modern"
        tree = self.query_one(WorkspaceTree)
        tree.show_root = False
        tree.auto_expand = False
        tree.root.expand()
        tree.focus()
        if tmux.inside_hub_server():
            tmux.hide_hub_status()
        self.refresh_data()
        self.set_interval(self.cfg.refresh_seconds, self.refresh_data)
        self.set_interval(0.3, self.check_request)

    @work(thread=True, exclusive=True, group="collect")
    def refresh_data(self) -> None:
        snap = self.collector.collect()
        self.call_from_thread(self.apply_snapshot, snap)

    def apply_snapshot(self, snap: Snapshot) -> None:
        self.snapshot = snap
        self.query_one("#empty").set_class(not snap.workspaces and not snap.elsewhere, "visible")
        waiting = sum(1 for ws in snap.workspaces for w in ws.windows if w.claude and w.claude.status == "waiting")
        summary = Text()
        if waiting:
            summary.append(f" ⏳ {waiting} waiting ", style=f"bold {T.BG} on {T.WARNING}")
            summary.append(" ")
        summary.append(f"{len(snap.workspaces)} workspaces ", style=T.MUTED)
        self.query_one("#header-right", Static).update(summary)
        self.render_tree()

    def _matches(self, *fields: str | None) -> bool:
        if not self.filter_text:
            return True
        needle = self.filter_text.lower()
        return any(needle in f.lower() for f in fields if f)

    def _workspace_label(self, ws: Workspace, name_w: int, path_w: int) -> Text:
        t = Text()
        t.append_text(status_text(ws.badge, with_name=False) if ws.badge else Text(" "))
        t.append(f" {ws.name:<{name_w}} ", style="bold")
        t.append(f"{shorten(ws.path):<{path_w}}  ", style=T.DIM)
        g = ws.git
        if g:
            t.append(f"⎇ {g.branch or '-'} ", style=T.PURPLE)
            if g.dirty:
                t.append(f"●{g.dirty} ", style=T.MODIFIED)
            if g.ahead:
                t.append(f"↑{g.ahead} ", style=T.INFO)
            if g.behind:
                t.append(f"↓{g.behind} ", style=T.INFO)
        return t

    def _window_label(self, w: Window, name_w: int) -> Text:
        t = Text()
        t.append("✻ " if w.claude else "  ", style=T.ACCENT)
        t.append(f"{w.name:<{name_w}} ", style=T.FG)
        s = w.claude
        if s is None:
            t.append(w.command, style=T.MUTED)
            return t
        t.append_text(status_text(s.status))
        t.append(f"  {age(s.timestamp)}", style=T.DIM)
        if s.message and s.status == "waiting":
            t.append(f"  {s.message.replace(chr(10), ' ')[:80]}", style=T.MODIFIED)
        return t

    def _elsewhere_label(self, s: Session, name_w: int) -> Text:
        t = Text()
        t.append_text(status_text(s.status, with_name=False))
        t.append(f" {s.name:<{name_w}} ", style="bold")
        t.append(f"{shorten(s.cwd)} ", style=T.DIM)
        if s.tmux_session:
            t.append(f"tmux:{s.tmux_session} ", style=T.INFO)
        t.append(age(s.timestamp), style=T.DIM)
        return t

    def _build_model(self) -> list[tuple]:
        snap = self.snapshot
        assert snap is not None
        visible = []
        for ws in snap.workspaces:
            branch = ws.git.branch if ws.git else None
            if self._matches(ws.name, ws.path, branch):
                visible.append((ws, ws.windows))
                continue
            wins = [w for w in ws.windows if self._matches(w.name, w.command, w.claude.message if w.claude else None)]
            if wins:
                visible.append((ws, wins))
        elsewhere = [s for s in snap.elsewhere if self._matches(s.name, s.cwd, s.message, "elsewhere")]

        name_w = min(28, max([len(ws.name) for ws, _ in visible] + [len(s.name) for s in elsewhere] + [4]))
        path_w = min(48, max([len(shorten(ws.path)) for ws, _ in visible] + [4]))
        win_w = min(16, max([len(w.name) for _, wins in visible for w in wins] + [4]))

        model = []
        for ws, wins in visible:
            children = [(("win", ws.name, w.window_id), self._window_label(w, win_w)) for w in wins]
            model.append((("ws", ws.name), self._workspace_label(ws, name_w, path_w), children))
        if elsewhere:
            label = Text("Outside Claude Hub", style=f"italic {T.MUTED}")
            children = [(("ext", s.session_id), self._elsewhere_label(s, name_w)) for s in elsewhere]
            model.append((("elsewhere",), label, children))
        return model

    def render_tree(self) -> None:
        if self.snapshot is None:
            return
        tree = self.query_one(WorkspaceTree)
        model = self._build_model()

        signature = [(k, str(lbl), [(ck, str(cl)) for ck, cl in ch]) for k, lbl, ch in model]
        if signature == self._signature:
            return
        self._signature = signature

        # Remember collapse state, including for nodes hidden by the filter.
        present = {n.data for n in tree.root.children}
        collapsed_now = {n.data for n in tree.root.children if n.allow_expand and not n.is_expanded}
        self.collapsed = (self.collapsed - present) | collapsed_now

        cursor_key = tree.cursor_node.data if tree.cursor_node else None
        tree.clear()
        lines: list[tuple] = []
        for key, label, children in model:
            lines.append(key)
            expanded = key not in self.collapsed
            node = tree.root.add(label, data=key, expand=expanded, allow_expand=bool(children))
            for ckey, clabel in children:
                node.add_leaf(clabel, data=ckey)
                if expanded:
                    lines.append(ckey)
        self._lines = lines
        if cursor_key in lines:
            tree.cursor_line = lines.index(cursor_key)
        elif cursor_key and cursor_key[0] == "win" and ("ws", cursor_key[1]) in lines:
            tree.cursor_line = lines.index(("ws", cursor_key[1]))
        elif lines:
            tree.cursor_line = 0

    def _workspace(self, name: str) -> Workspace | None:
        if self.snapshot is None:
            return None
        return next((w for w in self.snapshot.workspaces if w.name == name), None)

    def _cursor_key(self) -> tuple | None:
        node = self.query_one(WorkspaceTree).cursor_node
        return node.data if node is not None else None

    def _run(self, fn, *args) -> None:
        try:
            error = fn(*args)
        except tmux.NeedsAttach as e:
            self.exit(result=e.session)
            return
        if error:
            self.notify(error, severity="error")
        # The client has already switched away; refresh a moment later so it does not compete with the new shell.
        self.set_timer(0.3, self.refresh_data)

    @on(Tree.NodeSelected)
    def on_selected(self, event: Tree.NodeSelected) -> None:
        key = event.node.data
        if key is None:
            return
        if key[0] == "ws":
            self._run(tmux.switch_client, key[1])
        elif key[0] == "win":
            self._run(tmux.jump_to_window, key[1], key[2])
        elif key[0] == "ext":
            self.notify("This session runs outside claude-hub. Switch to it in its own terminal.")
        else:
            event.node.toggle()

    def action_new_window(self, kind: str) -> None:
        key = self._cursor_key()
        ws = self._workspace(key[1]) if key and key[0] in ("ws", "win") else None
        if ws is None:
            self.notify("Select a workspace first, or press n for a new one", severity="warning")
            return
        command = WINDOW_KINDS[kind]
        spare = self.snapshot.spares.get(ws.path) if self.snapshot and command is None else None
        self._run(tmux.new_window, ws.name, ws.path, kind, command, spare)

    def _relative(self, path: str) -> str:
        for root in self.cfg.roots:
            prefix = str(root).rstrip("/") + "/"
            if path.startswith(prefix):
                return path[len(prefix) :]
        return shorten(path)

    def _candidates(self) -> list[tuple[str, str, str]]:
        """(path, label, tag) for the picker: open workspaces, repos, recent dirs, other dirs."""
        found: dict[str, tuple[str, str, str]] = {}
        snap = self.snapshot
        for ws in snap.workspaces if snap else []:
            found[ws.path] = (ws.path, self._relative(ws.path), "open")
        dirs = self.collector.dirs
        for path, is_repo in dirs:
            if is_repo:
                found.setdefault(path, (path, self._relative(path), "repo"))
        for d in snap.recent_dirs if snap else []:
            found.setdefault(d, (d, self._relative(d), "recent"))
        for path, is_repo in dirs:
            found.setdefault(path, (path, self._relative(path), "dir"))
        return list(found.values())

    def _rescan_candidates(self) -> list[tuple[str, str, str]]:
        self.collector.rescan()
        return self._candidates()

    def action_new_workspace(self, query: str = "") -> None:
        def chosen(result: tuple[str, str] | None) -> None:
            self.collector.speculative = None
            if result is None:
                return
            path, kind = result
            workspaces = self.snapshot.workspaces if self.snapshot else []
            existing = next((ws for ws in workspaces if os.path.realpath(ws.path) == os.path.realpath(path)), None)
            if existing:
                self._run(tmux.switch_client, existing.name)
                return
            command = WINDOW_KINDS[kind]
            # Look up the pool fresh: the picker may have started a shell for this path a moment ago.
            spare = tmux.spares(tmux.list_all_panes()).get(path) if command is None else None
            name = tmux.unique_session_name(Path(path).name, {ws.name for ws in workspaces})
            self._run(tmux.create_workspace, name, path, kind, command, spare)

        self.push_screen(DirPicker(self._candidates(), self._rescan_candidates, query), chosen)

    def prewarm(self, path: str | None) -> None:
        if self.cfg.prewarm:
            self.run_worker(lambda: self.collector.prewarm(path), thread=True, group="prewarm")

    def check_request(self) -> None:
        """Pick up a request from `claude-hub <query>` run in another terminal."""
        try:
            query = tmux.REQUEST_FILE.read_text().strip()
            tmux.REQUEST_FILE.unlink()
        except OSError:
            return
        if len(self.screen_stack) == 1:
            self.action_new_workspace(query)

    def action_close(self) -> None:
        key = self._cursor_key()
        if key is None or key[0] not in ("ws", "win"):
            return
        ws = self._workspace(key[1])
        if ws is None:
            return
        if key[0] == "ws":
            question = f"Close workspace '{ws.name}' and all its {len(ws.windows)} window(s)?"
            action = lambda: tmux.kill_session(ws.name)  # noqa: E731
        else:
            win = next((w for w in ws.windows if w.window_id == key[2]), None)
            if win is None:
                return
            question = f"Close window '{win.name}' in '{ws.name}'?"
            action = lambda: tmux.kill_window(win.window_id)  # noqa: E731

        def done(ok: bool | None) -> None:
            if ok:
                self._run(action)

        self.push_screen(ConfirmScreen(question), done)

    def action_rename(self) -> None:
        key = self._cursor_key()
        if key is None or key[0] not in ("ws", "win"):
            return
        ws = self._workspace(key[1])
        if ws is None:
            return
        if key[0] == "ws":
            title, current = "Rename workspace", ws.name

            def apply(new: str) -> None:
                self._run(tmux.rename_session, ws.name, tmux.unique_session_name(new))
        else:
            win = next((w for w in ws.windows if w.window_id == key[2]), None)
            if win is None:
                return
            title, current = "Rename window", win.name

            def apply(new: str) -> None:
                self._run(tmux.rename_window, win.window_id, new)

        def done(new: str | None) -> None:
            if new and new != current:
                apply(new)

        self.push_screen(RenameScreen(title, current), done)

    def action_leave(self) -> None:
        if tmux.inside_hub_server():
            tmux.detach()
        else:
            self.exit()

    def action_filter(self) -> None:
        box = self.query_one("#filter", Input)
        box.add_class("visible")
        box.focus()

    def action_clear_filter(self) -> None:
        box = self.query_one("#filter", Input)
        box.value = ""
        box.remove_class("visible")
        self.query_one(WorkspaceTree).focus()

    @on(Input.Changed, "#filter")
    def on_filter_changed(self, event: Input.Changed) -> None:
        self.filter_text = event.value.strip()
        self.render_tree()

    @on(Input.Submitted, "#filter")
    def on_filter_submitted(self) -> None:
        self.query_one(WorkspaceTree).focus()

    def action_help(self) -> None:
        self.push_screen(HelpScreen())
