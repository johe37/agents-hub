"""VSCode Dark Modern colors, shared by the hub UI and the tmux status bar."""

BG = "#1F1F1F"
BAR = "#181818"
BORDER = "#2B2B2B"
FG = "#CCCCCC"
BRIGHT = "#FFFFFF"
MUTED = "#9D9D9D"
DIM = "#6E7681"
ACCENT = "#0078D4"
ACCENT_HOVER = "#026EC1"
INFO = "#3794FF"
SELECTION = "#04395E"
HOVER = "#2A2D2E"
WIDGET = "#222222"
INPUT = "#313131"
WARNING = "#CCA700"
ERROR = "#F85149"
SUCCESS = "#2EA043"
PURPLE = "#C586C0"
MODIFIED = "#E2C08D"


def textual_theme():
    from textual.theme import Theme

    return Theme(
        name="vscode-dark-modern",
        primary=ACCENT,
        secondary=INFO,
        accent=ACCENT,
        foreground=FG,
        background=BG,
        surface=BAR,
        panel=WIDGET,
        warning=WARNING,
        error=ERROR,
        success=SUCCESS,
        dark=True,
        variables={
            "block-cursor-background": SELECTION,
            "block-cursor-foreground": BRIGHT,
            "block-cursor-text-style": "none",
            "block-cursor-blurred-background": HOVER,
            "block-hover-background": HOVER,
            "footer-background": BAR,
            "footer-foreground": FG,
            "footer-item-background": BAR,
            "footer-key-foreground": FG,
            "footer-key-background": INPUT,
            "footer-description-foreground": MUTED,
            "footer-description-background": BAR,
            "input-selection-background": "#264F78",
            "border": ACCENT,
            "border-blurred": BORDER,
            "scrollbar": "#424242",
            "scrollbar-hover": "#4F4F4F",
            "scrollbar-background": BG,
        },
    )
