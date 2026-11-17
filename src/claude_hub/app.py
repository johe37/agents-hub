from rich.text import Text
from textual import events, on, work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import DataTable, Footer, Input, Static, TabbedContent, TabPane, Tree

from . import tmux
from .config import Config
from .snapshot import Collector, Repo, Snapshot
from .state import STATUS_STYLE, Session, age, remove_dead

HELP = """\
[b]claude-hub[/b]

[b]Global[/b]
  1 / 2 / Tab     switch view
  j k / arrows    move
  /               filter, Esc clears
  ?               this help
  q               quit

[b]Sessions[/b]
  Enter           jump to the Claude pane
  t               new shell window in the session's repo
  r               show the session's repo in the Repos view
  x               remove ended and stale sessions

[b]Repos[/b]
  Enter           jump to repo session (create if missing) or pane
  Space / l / h   expand / collapse
  c / e / t       new window with claude / nvim / shell
  H               show or hide repos without panes

[b]Mouse[/b]
  Click selects, click again (or double-click) jumps.
  Click ▸/▾ to expand. Click tabs and footer keys.
"""


def status_text(status: str | None, with_name: bool = True) -> Text:
    if status is None:
        return Text("")
    symbol, style = STATUS_STYLE.get(status, ("·", ""))
    return Text(f"{symbol} {status}" if with_name else symbol, style=style)


class HelpScreen(ModalScreen):
    DEFAULT_CSS = """
    HelpScreen { align: center middle; }
    HelpScreen > Static { width: auto; padding: 1 2; border: round $accent; background: $surface; }
    """

    def compose(self) -> ComposeResult:
        yield Static(HELP)

    def on_key(self, event: events.Key) -> None:
        event.stop()
        self.dismiss()

    def on_click(self) -> None:
        self.dismiss()


class SessionTable(DataTable):
    BINDINGS = [
        Binding("enter", "select_cursor", "Jump"),
        Binding("j", "cursor_down", show=False),
        Binding("k", "cursor_up", show=False),
        Binding("t", "app.session_terminal", "Terminal"),
        Binding("r", "app.goto_repo", "Repo"),
        Binding("x", "app.clean", "Clean up"),
    ]

    async def _on_click(self, event: events.Click) -> None:
        # Click selects, click on the selected row activates.
        event.prevent_default()
        row = event.style.meta.get("row")
        if row is None or row < 0:
            return
        if row == self.cursor_row:
            self.action_select_cursor()
        else:
            self.move_cursor(row=row)


class RepoTree(Tree):
    ICON_NODE = "▸ "
    ICON_NODE_EXPANDED = "▾ "
    BINDINGS = [
        Binding("enter", "select_cursor", "Jump"),
        Binding("space", "toggle_node", "Expand", show=False),
        Binding("j", "cursor_down", show=False),
        Binding("k", "cursor_up", show=False),
        Binding("l", "expand", show=False),
        Binding("h", "collapse", show=False),
        Binding("c", "app.repo_window('claude')", "Claude"),
        Binding("e", "app.repo_window('nvim')", "nvim"),
        Binding("t", "app.repo_window('shell')", "Shell"),
        Binding("H", "app.toggle_inactive", "Inactive"),
    ]

    async def _on_click(self, event: events.Click) -> None:
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
    TabbedContent { height: 1fr; }
    TabPane { padding: 0; }
    #filter { display: none; }
    #filter.visible { display: block; }
    """
    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("1", "show_tab('sessions')", show=False),
        Binding("2", "show_tab('repos')", show=False),
        Binding("tab", "next_tab", "View", priority=True),
        Binding("slash", "filter", "Filter"),
        Binding("question_mark", "help", "Help"),
        Binding("escape", "clear_filter", show=False),
    ]

    def __init__(self, cfg: Config, popup: bool = False) -> None:
        super().__init__()
        self.cfg = cfg
        self.popup = popup
        self.collector = Collector(cfg)
        self.snapshot: Snapshot | None = None
        self.filter_text = ""
        self.show_inactive = True
        self.collapsed: set[tuple] = set()
        self._session_rows: list[Session] = []
        self._tree_lines: list[tuple] = []
        self._tree_signature: object = None

    def compose(self) -> ComposeResult:
        with TabbedContent(initial="sessions"):
            with TabPane("1 Sessions", id="sessions"):
                yield SessionTable(cursor_type="row", zebra_stripes=True)
            with TabPane("2 Repos", id="repos"):
                yield RepoTree("repos")
        yield Input(placeholder="filter…", id="filter")
        yield Footer()

    def on_mount(self) -> None:
        table = self.query_one(SessionTable)
        table.add_columns("", "Repo", "Branch", "Status", "Age", "tmux", "Message")
        tree = self.query_one(RepoTree)
        tree.show_root = False
        tree.auto_expand = False
        tree.root.expand()
        table.focus()
        self.refresh_data()
        self.set_interval(self.cfg.refresh_seconds, self.refresh_data)

    @work(thread=True, exclusive=True, group="collect")
    def refresh_data(self) -> None:
        snap = self.collector.collect()
        self.call_from_thread(self.apply_snapshot, snap)

    def apply_snapshot(self, snap: Snapshot) -> None:
        self.snapshot = snap
        self.render_sessions()
        self.render_repos()

    def _matches(self, *fields: str | None) -> bool:
        if not self.filter_text:
            return True
        needle = self.filter_text.lower()
        return any(needle in f.lower() for f in fields if f)

    def render_sessions(self) -> None:
        if self.snapshot is None:
            return
        table = self.query_one(SessionTable)
        old_id = self.current_session().session_id if self.current_session() else None
        rows = [
            s
            for s in self.snapshot.sessions
            if self._matches(s.repo_name, s.branch, s.cwd, s.message, s.location, s.status)
        ]
        table.clear()
        for s in rows:
            symbol, style = STATUS_STYLE.get(s.status, ("·", ""))
            message = (s.message or "").replace("\n", " ")
            table.add_row(
                Text(symbol, style=style),
                Text(s.repo_name, style="bold" if s.alive else "dim"),
                Text(s.branch or "-", style="magenta" if s.alive else "dim"),
                Text(s.status, style=style),
                age(s.timestamp),
                s.location,
                message[:80],
                key=s.session_id,
            )
        self._session_rows = rows
        ids = [s.session_id for s in rows]
        if old_id in ids:
            table.move_cursor(row=ids.index(old_id), animate=False)

    def current_session(self) -> Session | None:
        table = self.query_one(SessionTable)
        if 0 <= table.cursor_row < len(self._session_rows):
            return self._session_rows[table.cursor_row]
        return None

    def _repo_label(self, repo: Repo, name_w: int, branch_w: int) -> Text:
        active = bool(repo.panes)
        st = repo.status
        t = Text()
        t.append(f"{repo.name:<{name_w}} ", style="bold")
        t.append(f"{st.branch or '-':<{branch_w}} ", style="magenta")
        t.append(f"✚{st.dirty:<3}" if st.dirty else "    ", style="yellow")
        ab = (f"↑{st.ahead}" if st.ahead else "") + (f"↓{st.behind}" if st.behind else "")
        t.append(f"{ab:<6}", style="cyan")
        t.append(f"▣ {len(repo.panes):<3}" if active else "     ", style="blue")
        if repo.badge:
            t.append_text(status_text(repo.badge, with_name=False))
        if not active:
            t.stylize("dim")
        return t

    def _pane_label(self, pane: tmux.Pane, loc_w: int, win_w: int) -> Text:
        snap = self.snapshot
        t = Text()
        t.append(f"{pane.location:<{loc_w}} ", style="cyan")
        t.append(f"{pane.window_name:<{win_w}} ")
        t.append(f"{pane.command:<10} ", style="green")
        session = snap.session_by_pane.get(pane.pane_id) if snap else None
        if session:
            t.append_text(status_text(session.status))
            if session.message and session.status == "waiting":
                t.append(f"  {session.message[:60]}", style="yellow")
        return t

    def render_repos(self) -> None:
        snap = self.snapshot
        if snap is None:
            return
        tree = self.query_one(RepoTree)

        repos = []
        for r in snap.repos:
            if not r.panes and not self.show_inactive:
                continue
            panes = [p for p in r.panes if self._matches(r.name, r.status.branch, p.location, p.window_name, p.command)]
            if self._matches(r.name, r.status.branch, r.root):
                panes = r.panes
            elif not panes:
                continue
            repos.append((r, panes))
        other = [p for p in snap.other_panes if self._matches("other", p.location, p.window_name, p.command, p.path)]

        all_panes = [p for _, ps in repos for p in ps] + other
        name_w = min(30, max((len(r.name) for r, _ in repos), default=4))
        branch_w = min(24, max((len(r.status.branch or "-") for r, _ in repos), default=4))
        loc_w = max((len(p.location) for p in all_panes), default=4)
        win_w = min(16, max((len(p.window_name) for p in all_panes), default=4))

        # Remember collapse state, including for nodes hidden by the filter.
        present = {n.data for n in tree.root.children}
        collapsed_now = {n.data for n in tree.root.children if n.allow_expand and not n.is_expanded}
        self.collapsed = (self.collapsed - present) | collapsed_now

        model = []
        for r, panes in repos:
            label = self._repo_label(r, name_w, branch_w)
            children = [(("pane", p.pane_id, r.root), self._pane_label(p, loc_w, win_w)) for p in panes]
            model.append((("repo", r.root), label, children))
        if other:
            label = Text("Other", style="italic")
            label.append(f"  ▣ {len(other)}", style="blue")
            children = [(("pane", p.pane_id, None), self._pane_label(p, loc_w, win_w)) for p in other]
            model.append((("other",), label, children))

        signature = [(k, str(lbl), [(ck, str(cl)) for ck, cl in ch]) for k, lbl, ch in model]
        if signature == self._tree_signature:
            return
        self._tree_signature = signature

        cursor_key = tree.cursor_node.data if tree.cursor_node else None
        tree.clear()
        lines: list[tuple] = []
        for key, label, children in model:
            lines.append(key)
            if not children:
                tree.root.add_leaf(label, data=key)
                continue
            expanded = key not in self.collapsed
            node = tree.root.add(label, data=key, expand=expanded)
            for ckey, clabel in children:
                node.add_leaf(clabel, data=ckey)
                if expanded:
                    lines.append(ckey)
        self._tree_lines = lines
        if cursor_key in lines:
            tree.cursor_line = lines.index(cursor_key)
        elif cursor_key and cursor_key[0] == "pane" and ("repo", cursor_key[2]) in lines:
            tree.cursor_line = lines.index(("repo", cursor_key[2]))

    def _after_jump(self, error: str | None) -> None:
        if error:
            self.notify(error, severity="error")
        elif self.popup:
            self.exit()
        self.refresh_data()

    @on(DataTable.RowSelected)
    def on_session_selected(self) -> None:
        s = self.current_session()
        if s is None:
            return
        if not s.tmux_pane:
            self.notify("Session has no tmux pane", severity="warning")
            return
        self._after_jump(tmux.jump_to_pane(s.tmux_pane))

    def _repo_by_root(self, root: str | None) -> Repo | None:
        if self.snapshot is None or root is None:
            return None
        return next((r for r in self.snapshot.repos if r.root == root), None)

    def _session_for_repo(self, repo: Repo) -> str | None:
        """Existing tmux session for a repo: one named after it, else the one with most of its panes."""
        own = tmux.session_name_for(repo.name)
        if own in tmux.list_sessions():
            return own
        if repo.panes:
            counts: dict[str, int] = {}
            for p in repo.panes:
                counts[p.session] = counts.get(p.session, 0) + 1
            return max(counts, key=counts.__getitem__)
        return None

    @on(Tree.NodeSelected)
    def on_tree_selected(self, event: Tree.NodeSelected) -> None:
        key = event.node.data
        if key is None:
            return
        if key[0] == "pane":
            self._after_jump(tmux.jump_to_pane(key[1]))
        elif key[0] == "repo":
            repo = self._repo_by_root(key[1])
            if repo is None:
                return
            session = self._session_for_repo(repo) or tmux.ensure_session(repo.name, repo.root)
            self._after_jump(tmux.switch_to_session(session))
        else:
            event.node.toggle()

    def action_repo_window(self, kind: str) -> None:
        node = self.query_one(RepoTree).cursor_node
        if node is None or node.data is None:
            return
        key = node.data
        repo = self._repo_by_root(key[1] if key[0] == "repo" else key[2] if key[0] == "pane" else None)
        if repo is not None:
            session = self._session_for_repo(repo) or tmux.ensure_session(repo.name, repo.root)
            cwd = repo.root
        elif key[0] == "pane" and self.snapshot:
            pane = next((p for p in self.snapshot.other_panes if p.pane_id == key[1]), None)
            if pane is None:
                return
            session, cwd = pane.session, pane.path
        else:
            return
        command = {"claude": "claude", "nvim": "nvim", "shell": None}[kind]
        self._after_jump(tmux.new_window(session, cwd, command, name=kind if command else None))

    def action_session_terminal(self) -> None:
        s = self.current_session()
        if s is None:
            return
        cwd = s.repo_root or s.cwd
        session = s.tmux_session if s.tmux_session in tmux.list_sessions() else None
        session = session or tmux.ensure_session(s.repo_name, cwd)
        self._after_jump(tmux.new_window(session, cwd))

    def action_goto_repo(self) -> None:
        s = self.current_session()
        if s is None or not s.repo_root:
            return
        self.action_show_tab("repos")
        key = ("repo", s.repo_root)
        if key in self._tree_lines:
            self.query_one(RepoTree).cursor_line = self._tree_lines.index(key)
        else:
            self.notify(f"{s.repo_name} is not in the repo list", severity="warning")

    def action_clean(self) -> None:
        if self.snapshot is None:
            return
        n = remove_dead(self.snapshot.sessions)
        self.notify(f"Removed {n} session(s)")
        self.refresh_data()

    def action_toggle_inactive(self) -> None:
        self.show_inactive = not self.show_inactive
        self.render_repos()

    def action_show_tab(self, tab: str) -> None:
        self.query_one(TabbedContent).active = tab
        self._focus_view(tab)

    def action_next_tab(self) -> None:
        tabs = self.query_one(TabbedContent)
        self.action_show_tab("repos" if tabs.active == "sessions" else "sessions")

    @on(TabbedContent.TabActivated)
    def on_tab_activated(self, event: TabbedContent.TabActivated) -> None:
        self._focus_view(event.pane.id)

    def _focus_view(self, tab: str | None) -> None:
        if tab == "repos":
            self.query_one(RepoTree).focus()
        else:
            self.query_one(SessionTable).focus()

    def action_filter(self) -> None:
        box = self.query_one("#filter", Input)
        box.add_class("visible")
        box.focus()

    def action_clear_filter(self) -> None:
        box = self.query_one("#filter", Input)
        box.value = ""
        box.remove_class("visible")
        self._focus_view(self.query_one(TabbedContent).active)

    @on(Input.Changed, "#filter")
    def on_filter_changed(self, event: Input.Changed) -> None:
        self.filter_text = event.value.strip()
        self.render_sessions()
        self.render_repos()

    @on(Input.Submitted, "#filter")
    def on_filter_submitted(self) -> None:
        self._focus_view(self.query_one(TabbedContent).active)

    def action_help(self) -> None:
        self.push_screen(HelpScreen())
