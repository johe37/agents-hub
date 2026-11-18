import os
from pathlib import Path

from rich.cells import cell_len
from rich.markdown import Markdown
from rich.text import Text
from textual import events, on, work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.markup import escape
from textual.screen import ModalScreen, Screen
from textual.suggester import Suggester
from textual.widgets import Button, Collapsible, Footer, Input, Label, OptionList, Static
from textual.widgets.option_list import Option

from . import fuzzy, tmux
from . import theme as T
from .config import Config
from .git import GitStatus
from .snapshot import Agent, Collector, Snapshot, Workspace, shorten
from .state import STATUS_STYLE, age
from .transcript import Entry, Transcript

WINDOW_KINDS = {"claude": "claude", "shell": None, "nvim": "nvim"}

HELP = """\
[b]claude-hub[/b]

[b]Agents[/b]
  Enter           open the conversation (or the workspace)
  n               open a workspace (fuzzy search or type a path)
  c / t / e       new Claude / terminal / nvim in the workspace
  x               close the agent or workspace
  R               rename the workspace
  r               reply: type a message, Enter sends it to the agent
  y / d           approve / deny a permission prompt
  j k / arrows    move
  PgUp / PgDn     scroll the detail panel
  /               filter, Esc clears
  q               leave (everything keeps running)

[b]In a conversation[/b]
  r               reply, Enter sends
  y / d           approve / deny a permission prompt
  t               the agent's terminal, for when you need it
  Esc / q         back to the list
  j k / PgUp PgDn scroll, End jumps to the newest step
  Click a tool call to see its result.

[b]From any workspace[/b]
  Click ◀ Claude Hub in the top bar, or use your tmux prefix + s
  and pick the _hub session.

[b]Mouse[/b]
  Click selects, click again (or double-click) opens.
  Click a terminal in the detail panel to jump to it.
"""


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


GROUPS = (("waiting", "NEEDS YOU"), ("working", "WORKING"), ("idle", "IDLE"))
WAITING_REASON = {
    "permission_prompt": "Needs your permission",
    "idle_prompt": "Waiting for your input",
    "elicitation_dialog": "Asks you a question",
}
# Keys that answer Claude Code's permission dialog: Enter takes the highlighted "Yes", Escape declines.
APPROVE_KEYS = ("Enter",)
DENY_KEYS = ("Escape",)
FILE_STYLE = {"?": T.SUCCESS, "A": T.SUCCESS, "D": T.ERROR, "R": T.INFO, "U": T.ERROR}
MAX_FILES = 20
MAX_MESSAGE_LINES = 40
MAX_ENTRIES = 150


def git_text(g: GitStatus | None) -> Text:
    t = Text()
    if not g:
        return t
    t.append(f"⎇ {g.branch or '-'}", style=T.PURPLE)
    if g.dirty:
        t.append(f" ●{g.dirty}", style=T.MODIFIED)
    if g.added:
        t.append(f" +{g.added}", style=T.SUCCESS)
    if g.removed:
        t.append(f" −{g.removed}", style=T.ERROR)
    if g.ahead:
        t.append(f" ↑{g.ahead}", style=T.INFO)
    if g.behind:
        t.append(f" ↓{g.behind}", style=T.INFO)
    return t


def status_line(a: Agent) -> tuple[Text, Text]:
    """(headline, detail) describing what the agent does right now."""
    symbol, style = STATUS_STYLE[a.status]
    s, t = a.session, a.transcript
    head = Text(f"{symbol} ", style=style)
    detail = Text()
    when = f" · {age(s.timestamp)}" if s else ""
    if a.status == "waiting":
        head.append(WAITING_REASON.get(s.notification_type or "", "Waiting for you") + when, style=f"bold {T.WARNING}")
        if s.message:
            detail.append(s.message, style=T.MODIFIED)
        # The hook keeps only the latest event, so the tool behind a permission prompt comes from the transcript.
        if s.notification_type == "permission_prompt" and t and t.last_tool:
            detail.append(("\n" if detail else "") + t.last_tool[:200], style=T.FG)
        if a.target:
            detail.append("\ny approves · d denies · r replies · Enter opens the conversation", style=T.MUTED)
    elif a.status == "working":
        head.append("Working" + when, style=T.INFO)
        tool = (s.tool if s else None) or (t.last_tool if t else None)
        if tool:
            detail.append(tool[:120], style=T.FG)
    elif a.status == "idle":
        head.append("Idle" + when, style=T.SUCCESS)
        detail.append("Done with its turn. Open it to continue.", style=T.MUTED)
    else:
        head.append("No status", style=T.DIM)
        detail.append("Started before claude-hub hooks were installed.", style=T.MUTED)
    return head, detail


class AgentList(OptionList):
    BINDINGS = [
        Binding("enter", "select", "Open"),
        Binding("n", "app.new_workspace", "New"),
        Binding("c", "app.new_window('claude')", "Claude"),
        Binding("t", "app.new_window('shell')", "Terminal"),
        Binding("e", "app.new_window('nvim')", "nvim"),
        Binding("x", "app.close", "Close"),
        Binding("R", "app.rename", "Rename"),
        Binding("r", "app.reply", "Reply"),
        Binding("y", "app.approve", "Approve"),
        Binding("d", "app.deny", "Deny"),
        Binding("j", "cursor_down", show=False),
        Binding("k", "cursor_up", show=False),
    ]

    async def _on_click(self, event: events.Click) -> None:
        # Click selects, click on the selected row opens it.
        index = event.style.meta.get("option")
        if index is None or self._options[index].disabled:
            return
        if index == self.highlighted or event.chain == 2:
            self.highlighted = index
            self.action_select()
        else:
            self.highlighted = index


class DetailPanel(VerticalScroll):
    DEFAULT_CSS = """
    DetailPanel { width: 100%; height: 1fr; padding: 0 2; background: $background; }
    DetailPanel #title { color: #FFFFFF; text-style: bold; }
    DetailPanel #meta { color: $text-muted; }
    DetailPanel #state { margin-top: 1; }
    DetailPanel .heading { color: $text-muted; text-style: bold; margin-top: 1; }
    """
    SECTIONS = ("state", "request", "message", "files", "terminals", "session")
    HEADINGS = {"request": "Your last request", "message": "Last message", "terminals": "Terminals", "session": "Session"}

    def compose(self) -> ComposeResult:
        yield Static("", id="title")
        yield Static("", id="meta")
        for section in self.SECTIONS:
            if section != "state":
                yield Static("", id=f"{section}-h", classes="heading")
            yield Static("", id=section)

    def _set(self, section: str, content, heading: str | None = None) -> None:
        widget = self.query_one(f"#{section}", Static)
        shown = bool(content)
        widget.display = shown
        if section != "state":
            h = self.query_one(f"#{section}-h", Static)
            h.display = shown
            if shown:
                self._update(h, heading or self.HEADINGS.get(section, section))
        if shown:
            self._update(widget, content)

    @staticmethod
    def _update(widget: Static, content) -> None:
        key = content.markup if isinstance(content, Markdown) else str(content)
        if getattr(widget, "_hub_key", None) != key:
            widget._hub_key = key
            widget.update(content)

    def show_empty(self) -> None:
        self._set_all({"title": "No workspaces yet", "meta": "Press n to open one, then c to start Claude in it."})

    def show_agent(self, a: Agent) -> None:
        meta = Text()
        meta.append(a.name, style=f"bold {T.FG}")
        meta.append(f"  {shorten(a.path)}  ", style=T.DIM)
        meta.append_text(git_text(a.git))
        if not a.in_hub:
            where = f"tmux session {a.session.tmux_session}" if a.session and a.session.tmux_session else "its own terminal"
            meta.append(f"\nRuns outside Claude Hub, in {where}.", style=f"italic {T.MUTED}")
        head, detail = status_line(a)
        state = Text.assemble(head, "\n", detail) if detail else head
        t = a.transcript
        request = Text(t.last_prompt[:400], style=T.FG) if t and t.last_prompt else None
        message = None
        if t and t.last_text:
            lines = t.last_text.splitlines()
            message = Markdown("\n".join(lines[-MAX_MESSAGE_LINES:]), code_theme="github-dark")
        session = None
        if a.session:
            parts = []
            if t and t.model:
                parts.append(t.model)
            if t and t.turns:
                parts.append(f"{t.turns} turn{'s' if t.turns != 1 else ''}")
            if t and t.cost_usd:
                parts.append(f"${t.cost_usd:.2f}")
            if a.session.permission_mode:
                parts.append(f"{a.session.permission_mode} mode")
            parts.append(a.session.session_id[:8])
            session = Text(" · ".join(parts), style=T.DIM)
        self._set_all({
            "title": Text(f"✻ {a.title}", style=f"bold {T.BRIGHT}"),
            "meta": meta,
            "state": state,
            "request": request,
            "message": message,
            "files": self._files(a.git),
            "terminals": self._terminals(a.workspace, a.window.window_id if a.window else None),
            "session": session,
        })

    def show_workspace(self, ws: Workspace) -> None:
        meta = Text()
        meta.append(f"{shorten(ws.path)}  ", style=T.DIM)
        meta.append_text(git_text(ws.git))
        state = Text()
        state.append("No Claude running here. ", style=T.MUTED)
        state.append("c", style=f"bold {T.FG}")
        state.append(" starts one.", style=T.MUTED)
        self._set_all({
            "title": Text(ws.name, style=f"bold {T.BRIGHT}"),
            "meta": meta,
            "state": state,
            "files": self._files(ws.git),
            "terminals": self._terminals(ws, None),
        })

    def _set_all(self, content: dict) -> None:
        self._update(self.query_one("#title", Static), content.get("title", ""))
        self._update(self.query_one("#meta", Static), content.get("meta", ""))
        for section in self.SECTIONS:
            value = content.get(section)
            heading = None
            if section == "files" and value:
                value, heading = value
            self._set(section, value, heading)

    @staticmethod
    def _files(g: GitStatus | None) -> tuple[Text, str] | None:
        if not g or not g.files:
            return None
        t = Text()
        for i, (status, path) in enumerate(g.files[:MAX_FILES]):
            letter = next((c for c in status if c != "."), "M")
            t.append(f"{letter}  ", style=FILE_STYLE.get(letter, T.MODIFIED))
            t.append(path, style=T.FG)
            if i < len(g.files) - 1:
                t.append("\n")
        if len(g.files) > MAX_FILES:
            t.append(f"… {len(g.files) - MAX_FILES} more", style=T.DIM)
        return t, f"Changed files ({len(g.files)})"

    @staticmethod
    def _terminals(ws: Workspace | None, own: str | None) -> str | None:
        if ws is None:
            return None
        lines = []
        for w in ws.windows:
            kind = "✻ " if w.claude or w.command == "claude" else "  "
            label = escape(f"{w.index} {kind}{w.name}")
            command = "" if w.command in (w.name, "claude") else f"  [{T.DIM}]{escape(w.command)}[/]"
            marker = f"  [{T.DIM}]◂ this agent[/]" if w.window_id == own else ""
            lines.append(f"[@click=app.open_window({ws.name!r}, {w.window_id!r})]{label}[/]{command}{marker}")
        lines.append(f"[{T.MUTED}]t new terminal · e nvim · c Claude[/]")
        return "\n".join(lines)


def needs_permission(a: Agent) -> bool:
    return a.status == "waiting" and a.session is not None and a.session.notification_type == "permission_prompt"


def reply_placeholder(a: Agent) -> str:
    if needs_permission(a):
        return f"Answer {a.name}: y approves, d denies, or write what to do instead"
    return f"Message to {a.name} · r to type, Enter sends"


def tool_title(e: Entry) -> str:
    mark = "…" if e.result is None else ("✗" if e.is_error else "✓")
    return f"{mark} {e.text[:140]}"


class ConversationScreen(Screen):
    """The agent's conversation, read from its transcript. Replies go to its terminal."""

    DEFAULT_CSS = """
    ConversationScreen { background: $background; }
    #conv-top { height: 2; background: $surface; border-bottom: solid #2B2B2B; padding: 0 1; }
    #log { height: 1fr; padding: 0 2; scrollbar-size: 1 1; }
    #log .user { background: #2A2D2E; border-left: thick #0078D4; padding: 0 1; margin: 1 0 0 0; }
    #log .assistant { padding: 0 1; margin: 1 0 0 0; }
    #log .note { color: $text-muted; text-style: italic; padding: 0 1; }
    #log Collapsible { margin: 0 0 0 1; padding: 0; border: none; background: $background; }
    #log CollapsibleTitle { color: #9D9D9D; padding: 0; }
    #log CollapsibleTitle:hover { color: #FFFFFF; background: #2A2D2E; }
    #log Collapsible.error CollapsibleTitle { color: #F85149; }
    #log Collapsible > Contents { padding: 0 0 0 3; }
    #log .result { color: #9D9D9D; }
    #conv-state { height: auto; padding: 0 2; margin-top: 1; }
    #ask { margin: 0 1; }
    """
    BINDINGS = [
        Binding("escape", "back", "Back"),
        Binding("q", "back", show=False),
        Binding("r", "reply", "Reply"),
        Binding("y", "approve", "Approve"),
        Binding("d", "deny", "Deny"),
        Binding("t", "terminal", "Terminal"),
        Binding("end", "bottom", "Newest"),
        Binding("j", "scroll_log(1)", show=False),
        Binding("k", "scroll_log(-1)", show=False),
    ]

    def __init__(self, key: str) -> None:
        super().__init__()
        self.key = key
        self.agent_now: Agent | None = None
        self._version = -1
        self._mounted = 0
        self._pending: dict[int, Collapsible] = {}

    def compose(self) -> ComposeResult:
        yield Static("", id="conv-top")
        yield VerticalScroll(id="log")
        yield Static("", id="conv-state")
        yield Input(placeholder="", id="ask")
        yield Footer()

    def on_mount(self) -> None:
        self.query_one("#log").focus()
        self.tick()
        self.set_interval(1.0, self.tick)

    @work(thread=True, exclusive=True, group="conversation")
    def tick(self) -> None:
        app: HubApp = self.app
        agent = app.agent(self.key)
        path = agent.session.transcript_path if agent and agent.session else None
        transcript = app.collector.transcripts.get(path)
        app.call_from_thread(self.apply, agent, transcript)

    def apply(self, agent: Agent | None, t: Transcript | None) -> None:
        if agent is not None:
            self.agent_now = agent
        a = self.agent_now
        top = Text()
        if a is None:
            top.append("This agent is gone.", style=T.MUTED)
        else:
            top.append(f" ✻ {a.title}", style=f"bold {T.BRIGHT}")
            top.append(f"   {a.name}  ", style=T.MUTED)
            top.append_text(git_text(a.git))
            if agent is None:
                top.append("   ended", style=T.DIM)
        self.query_one("#conv-top", Static).update(top)
        if a is not None:
            head, detail = status_line(a)
            self.query_one("#conv-state", Static).update(Text.assemble(head, "  ", detail.split("\n")[0] if detail else ""))
            self.query_one("#ask", Input).placeholder = reply_placeholder(a)
        self.refresh_bindings()
        if t is None or t.version == self._version:
            return
        self._version = t.version
        self._show_entries(t)

    def _show_entries(self, t: Transcript) -> None:
        log = self.query_one("#log", VerticalScroll)
        first = self._mounted == 0
        at_bottom = first or log.scroll_y >= log.max_scroll_y - 1

        for index, widget in list(self._pending.items()):
            e = t.entries[index]
            if e.result is not None:
                widget.title = tool_title(e)
                widget.set_class(e.is_error, "error")
                widget.query_one(".result", Static).update(Text(e.result or "(no output)"))
                del self._pending[index]

        start = self._mounted
        widgets = []
        if first and len(t.entries) > MAX_ENTRIES:
            start = len(t.entries) - MAX_ENTRIES
            widgets.append(Static(f"… {start} earlier steps not shown", classes="note"))
        for index in range(start, len(t.entries)):
            e = t.entries[index]
            if e.kind == "user":
                widgets.append(Static(Text(e.text), classes="user"))
            elif e.kind == "assistant":
                widgets.append(Static(Markdown(e.text, code_theme="github-dark"), classes="assistant"))
            else:
                body = Static(Text(e.result if e.result is not None else "running…"), classes="result")
                box = Collapsible(body, title=tool_title(e), collapsed=True, classes="error" if e.is_error else "")
                if e.result is None:
                    self._pending[index] = box
                widgets.append(box)
        self._mounted = len(t.entries)
        if widgets:
            log.mount(*widgets)
            if at_bottom:
                self.call_after_refresh(log.scroll_end, animate=False)

    def check_action(self, action: str, parameters: tuple) -> bool:
        # False hides the key in the footer; None would only grey it out.
        a = self.agent_now
        if action in ("approve", "deny"):
            return bool(a and needs_permission(a))
        if action == "reply":
            return bool(a and a.target)
        if action == "terminal":
            return bool(a and a.in_hub)
        return True

    def action_back(self) -> None:
        if self.query_one("#ask", Input).has_focus:
            self.query_one("#log").focus()
        else:
            self.dismiss()

    def action_reply(self) -> None:
        if self.agent_now and self.agent_now.target:
            self.query_one("#ask", Input).focus()

    def action_approve(self) -> None:
        if self.agent_now and needs_permission(self.agent_now):
            self.app._send(self.agent_now, "Approved", tmux.send_keys, *APPROVE_KEYS)

    def action_deny(self) -> None:
        if self.agent_now and needs_permission(self.agent_now):
            self.app._send(self.agent_now, "Denied", tmux.send_keys, *DENY_KEYS)

    def action_terminal(self) -> None:
        if self.agent_now:
            self.app.open_terminal(self.agent_now)

    def action_bottom(self) -> None:
        self.query_one("#log", VerticalScroll).scroll_end(animate=False)

    def action_scroll_log(self, direction: int) -> None:
        log = self.query_one("#log", VerticalScroll)
        log.scroll_relative(y=3 * direction, animate=False)

    @on(Input.Submitted, "#ask")
    def on_ask(self, event: Input.Submitted) -> None:
        text = event.value.strip()
        self.query_one("#ask", Input).value = ""
        self.query_one("#log").focus()
        if text and self.agent_now:
            self.app.send_reply(self.agent_now, text)


class HubApp(App):
    TITLE = "claude-hub"
    ENABLE_COMMAND_PALETTE = False
    CSS = """
    #header { height: 2; background: $surface; border-bottom: solid #2B2B2B; }
    #header-left { width: 1fr; }
    #header-right { width: auto; }
    #main { height: 1fr; }
    #side { width: 55%; min-width: 40; border-left: solid #2B2B2B; }
    #reply { display: none; margin: 0 1; }
    #reply.visible { display: block; }
    AgentList { width: 1fr; height: 1fr; background: $background; border: none; padding: 0 1; scrollbar-size: 1 1; }
    AgentList > .option-list--option-disabled { color: #9D9D9D; text-style: bold; }
    Footer { border-top: none; }
    Input { background: #313131; border: tall #3C3C3C; }
    Input:focus { border: tall #0078D4; }
    #filter { display: none; }
    #filter.visible { display: block; }
    """
    BINDINGS = [
        Binding("q", "leave", "Leave"),
        Binding("slash", "filter", "Filter"),
        Binding("question_mark", "help", "Help"),
        Binding("escape", "clear_filter", show=False),
        Binding("pagedown", "scroll_detail(1)", show=False),
        Binding("pageup", "scroll_detail(-1)", show=False),
    ]

    def __init__(self, cfg: Config) -> None:
        super().__init__()
        self.cfg = cfg
        self.collector = Collector(cfg)
        self.snapshot: Snapshot | None = None
        self.filter_text = ""
        self._rows: dict[str, Agent | Workspace] = {}
        self._signature: object = None

    def compose(self) -> ComposeResult:
        title = Text()
        title.append(" ◆ ", style=f"bold {T.ACCENT}")
        title.append("Agents", style=f"bold {T.BRIGHT}")
        title.append("   click ◀ Claude Hub in any workspace to come back", style=T.MUTED)
        with Horizontal(id="header"):
            yield Static(title, id="header-left")
            yield Static("", id="header-right")
        with Horizontal(id="main"):
            yield AgentList(id="list")
            with Vertical(id="side"):
                yield DetailPanel(id="detail")
                yield Input(placeholder="", id="reply")
        yield Input(placeholder="filter…", id="filter")
        yield Footer()

    def on_mount(self) -> None:
        self.register_theme(T.textual_theme())
        self.theme = "vscode-dark-modern"
        self.query_one(AgentList).focus()
        if tmux.inside_hub_server():
            tmux.hide_hub_status()
        self.refresh_data()
        self.set_interval(self.cfg.refresh_seconds, self.refresh_data)
        self.set_interval(0.3, self.check_request)

    def on_resize(self, event: events.Resize) -> None:
        self.query_one("#side").display = event.size.width >= 100
        self.render_list()

    @work(thread=True, exclusive=True, group="collect")
    def refresh_data(self) -> None:
        snap = self.collector.collect()
        self.call_from_thread(self.apply_snapshot, snap)

    def apply_snapshot(self, snap: Snapshot) -> None:
        self.snapshot = snap
        counts = {status: sum(1 for a in snap.agents if a.status == status) for status, _ in GROUPS}
        summary = Text()
        if counts["waiting"]:
            summary.append(f" ⏳ {counts['waiting']} need you ", style=f"bold {T.BG} on {T.WARNING}")
            summary.append("  ")
        if counts["working"]:
            summary.append(f"● {counts['working']} working  ", style=T.INFO)
        if counts["idle"]:
            summary.append(f"○ {counts['idle']} idle  ", style=T.SUCCESS)
        n = len(snap.workspaces)
        summary.append(f"{n} workspace{'s' if n != 1 else ''} ", style=T.MUTED)
        self.query_one("#header-right", Static).update(summary)
        self.render_list()

    def _matches(self, *fields: str | None) -> bool:
        if not self.filter_text:
            return True
        needle = self.filter_text.lower()
        return any(needle in f.lower() for f in fields if f)

    def _agent_row(self, a: Agent, width: int) -> Text:
        symbol, style = STATUS_STYLE[a.status]
        right = age(a.timestamp) if a.session else ""
        title = Text(a.title, style=f"bold {T.BRIGHT}" if a.status == "waiting" else T.FG)
        title.truncate(max(8, width - cell_len(symbol) - 1 - len(right) - 3), overflow="ellipsis", pad=True)
        first = Text(f"{symbol} ", style=style)
        first.append_text(title)
        first.append(f"  {right}", style=T.DIM)

        second = Text("  ")
        second.append(a.name, style=T.MUTED)
        if a.git:
            second.append("  ")
            second.append_text(git_text(a.git))
        s = a.session
        if a.status == "waiting" and s:
            reason = WAITING_REASON.get(s.notification_type or "")
            second.append(f"  {reason or s.message or 'Waiting for you'}", style=T.MODIFIED)
        elif a.status == "working" and s and s.tool:
            second.append(f"  {s.tool}", style=T.DIM)
        if not a.in_hub:
            second.append("  outside hub", style=f"italic {T.DIM}")
        second.truncate(width, overflow="ellipsis")
        return Text.assemble(first, "\n", second)

    def _workspace_row(self, ws: Workspace, width: int) -> Text:
        first = Text("▪ ", style=T.DIM)
        first.append(ws.name, style=T.FG)
        first.append(f"  {shorten(ws.path)}", style=T.DIM)
        first.truncate(width, overflow="ellipsis")
        n = len(ws.windows)
        second = Text("  ")
        second.append(f"{n} terminal{'s' if n != 1 else ''}", style=T.MUTED)
        if ws.git:
            second.append("  ")
            second.append_text(git_text(ws.git))
        second.truncate(width, overflow="ellipsis")
        return Text.assemble(first, "\n", second)

    def _visible(self) -> tuple[list[tuple[str, list[Agent]]], list[Workspace]]:
        snap = self.snapshot
        assert snap is not None
        groups = []
        for status, heading in GROUPS:
            members = [
                a
                for a in snap.agents
                if (a.status == status or (status == "idle" and a.status == "unknown"))
                and self._matches(
                    a.title, a.name, a.path, a.git.branch if a.git else None,
                    a.session.message if a.session else None,
                    a.transcript.last_prompt if a.transcript else None,
                )
            ]
            if members:
                groups.append((heading, members))
        workspaces = [
            ws for ws in snap.idle_workspaces if self._matches(ws.name, ws.path, ws.git.branch if ws.git else None)
        ]
        return groups, workspaces

    def render_list(self) -> None:
        if self.snapshot is None:
            return
        listing = self.query_one(AgentList)
        width = listing.content_size.width or 60
        groups, workspaces = self._visible()

        options: list[Option] = []
        rows: dict[str, Agent | Workspace] = {}
        for i, (heading, members) in enumerate(groups):
            options.append(Option(Text(("\n" if i else "") + f"{heading} · {len(members)}"), disabled=True))
            for a in members:
                options.append(Option(self._agent_row(a, width), id=a.key))
                rows[a.key] = a
        if workspaces:
            options.append(Option(Text(("\n" if groups else "") + f"WORKSPACES · {len(workspaces)}"), disabled=True))
            for ws in workspaces:
                key = f"ws:{ws.name}"
                options.append(Option(self._workspace_row(ws, width), id=key))
                rows[key] = ws
        if not options:
            options.append(Option(Text("Nothing here yet. Press n to open a workspace.", style=T.MUTED), disabled=True))

        signature = [(o.id, str(o.prompt)) for o in options]
        self._rows = rows
        if signature == self._signature:
            self.render_detail()
            return
        self._signature = signature

        current = listing.highlighted_option.id if listing.highlighted_option else None
        listing.clear_options()
        listing.add_options(options)
        enabled = [i for i, o in enumerate(options) if not o.disabled]
        if enabled:
            wanted = next((i for i in enabled if options[i].id == current), enabled[0])
            listing.highlighted = wanted
        self.render_detail()

    def _current(self) -> Agent | Workspace | None:
        option = self.query_one(AgentList).highlighted_option
        return self._rows.get(option.id) if option and option.id else None

    def render_detail(self) -> None:
        panel = self.query_one(DetailPanel)
        row = self._current()
        if isinstance(row, Agent):
            panel.show_agent(row)
        elif isinstance(row, Workspace):
            panel.show_workspace(row)
        else:
            panel.show_empty()
        reply = self.query_one("#reply", Input)
        agent = row if isinstance(row, Agent) and row.target else None
        reply.set_class(agent is not None, "visible")
        if agent is not None:
            reply.placeholder = reply_placeholder(agent)
        self.refresh_bindings()

    @staticmethod
    def _needs_permission(a: Agent) -> bool:
        return needs_permission(a)

    def agent(self, key: str) -> Agent | None:
        if self.snapshot is None:
            return None
        return next((a for a in self.snapshot.agents if a.key == key), None)

    def check_action(self, action: str, parameters: tuple) -> bool:
        if action in ("filter", "help") and isinstance(self.screen, ConversationScreen):
            return action == "help"
        if action in ("approve", "deny"):
            row = self._current()
            return isinstance(row, Agent) and self._needs_permission(row)
        if action == "reply":
            row = self._current()
            return isinstance(row, Agent) and bool(row.target)
        return True

    def _send(self, agent: Agent, what: str, fn, *args) -> None:
        if agent.target is None:
            self.notify("The hub does not know where this agent reads its input.", severity="warning")
            return
        socket, pane = agent.target
        error = fn(socket, pane, *args)
        if error:
            self.notify(error, severity="error")
            return
        self.notify(f"{what} → {agent.title}")
        self.set_timer(0.8, self.refresh_data)

    def action_approve(self) -> None:
        row = self._current()
        if isinstance(row, Agent) and self._needs_permission(row):
            self._send(row, "Approved", tmux.send_keys, *APPROVE_KEYS)

    def action_deny(self) -> None:
        row = self._current()
        if isinstance(row, Agent) and self._needs_permission(row):
            self._send(row, "Denied", tmux.send_keys, *DENY_KEYS)

    def action_reply(self) -> None:
        row = self._current()
        if isinstance(row, Agent) and row.target:
            self.query_one("#reply", Input).focus()

    def send_reply(self, agent: Agent, text: str) -> None:
        """Interpret a typed reply: y/n answers a permission prompt, anything else is typed to the agent."""
        if self._needs_permission(agent) and text.lower() in ("y", "yes"):
            self._send(agent, "Approved", tmux.send_keys, *APPROVE_KEYS)
        elif self._needs_permission(agent) and text.lower() in ("n", "no", "d"):
            self._send(agent, "Denied", tmux.send_keys, *DENY_KEYS)
        else:
            self._send(agent, "Sent", tmux.send_text, text)

    def open_terminal(self, agent: Agent) -> None:
        if agent.in_hub:
            self._run(tmux.jump_to_window, agent.workspace.name, agent.window.window_id)
        else:
            where = f"tmux session '{agent.session.tmux_session}'" if agent.session and agent.session.tmux_session else "its own terminal"
            self.notify(f"This agent runs outside Claude Hub, in {where}. Switch to it there.")

    @on(Input.Submitted, "#reply")
    def on_reply_submitted(self, event: Input.Submitted) -> None:
        text = event.value.strip()
        row = self._current()
        self.query_one("#reply", Input).value = ""
        self.query_one(AgentList).focus()
        if text and isinstance(row, Agent):
            self.send_reply(row, text)

    @on(OptionList.OptionHighlighted)
    def on_highlighted(self) -> None:
        self.render_detail()

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

    @on(OptionList.OptionSelected)
    def on_selected(self) -> None:
        row = self._current()
        if isinstance(row, Agent):
            self.push_screen(ConversationScreen(row.key))
        elif isinstance(row, Workspace):
            self._run(tmux.switch_client, row.name)

    def action_open_window(self, session: str, window_id: str) -> None:
        self._run(tmux.jump_to_window, session, window_id)

    def action_scroll_detail(self, direction: int) -> None:
        panel = self.query_one(DetailPanel)
        if direction > 0:
            panel.scroll_page_down()
        else:
            panel.scroll_page_up()

    def _current_workspace(self) -> Workspace | None:
        row = self._current()
        if isinstance(row, Agent):
            return row.workspace
        return row if isinstance(row, Workspace) else None

    def action_new_window(self, kind: str) -> None:
        ws = self._current_workspace()
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
        row = self._current()
        if isinstance(row, Agent):
            if not row.in_hub:
                self.notify("This agent runs outside Claude Hub. Close it in its own terminal.", severity="warning")
                return
            question = f"Close agent '{row.title}' in '{row.workspace.name}'? This ends its Claude session."
            window_id = row.window.window_id
            action = lambda: tmux.kill_window(window_id)  # noqa: E731
        elif isinstance(row, Workspace):
            question = f"Close workspace '{row.name}' and all its {len(row.windows)} window(s)?"
            name = row.name
            action = lambda: tmux.kill_session(name)  # noqa: E731
        else:
            return

        def done(ok: bool | None) -> None:
            if ok:
                self._run(action)

        self.push_screen(ConfirmScreen(question), done)

    def action_rename(self) -> None:
        ws = self._current_workspace()
        if ws is None:
            return
        current = ws.name

        def done(new: str | None) -> None:
            if new and new != current:
                self._run(tmux.rename_session, current, tmux.unique_session_name(new))

        self.push_screen(RenameScreen("Rename workspace", current), done)

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
        if self.query_one("#reply", Input).has_focus:
            self.query_one(AgentList).focus()
            return
        box = self.query_one("#filter", Input)
        box.value = ""
        box.remove_class("visible")
        self.query_one(AgentList).focus()

    @on(Input.Changed, "#filter")
    def on_filter_changed(self, event: Input.Changed) -> None:
        self.filter_text = event.value.strip()
        self.render_list()

    @on(Input.Submitted, "#filter")
    def on_filter_submitted(self) -> None:
        self.query_one(AgentList).focus()

    def action_help(self) -> None:
        self.push_screen(HelpScreen())
