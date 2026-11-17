# Uppdrag: bygg "claude-hub" – en central TUI för mina Claude-sessioner och git-repon

## Bakgrund
Jag kör Claude Code i terminalen på min utvecklingsmaskin med tmux. Varje git-repo
jag har öppet har sin egen tmux-session, och jag kan ha flera Claude Code-instanser igång
samtidigt i olika repon. Jag vill ha ETT verktyg med två vyer:
1. En vy över alla mina Claude-sessioner och deras status.
2. En vy över git-repona jag jobbar i och vilka tmux-fönster/panes som hör till dem.
Verktyget ska gå att styra både med tangentbord och mus.
3. Till Claude sessionerna som är igång ska det vara enkleklt att öppna en terminal för
att eventuellt testa något i projektet i fråga

## Arkitektur

### Statusrapportering via hooks (inte screen-scraping)
- Använd Claude Code hooks (SessionStart, UserPromptSubmit, PreToolUse,
  Notification, Stop, SessionEnd) som anropar `claude-hub report`.
- `report` läser hookens JSON från stdin och skriver en statusfil per session till
  `~/.local/state/claude-hub/<session_id>.json`.
- Statusfilen innehåller minst: session_id, cwd, git-repo-rot och gren (om cwd är
  i ett repo), tmux-session/fönster/pane (från $TMUX_PANE om den finns), status,
  senaste hook-händelse, tidsstämpel, transcript_path och, för Notification,
  meddelandetexten.

### Statusmodell
- `working`: prompt skickad eller verktyg körs
- `waiting`: Notification, väntar på mig
- `idle`: Stop, klar med sin tur
- `ended`: SessionEnd
- `stale`: ingen uppdatering på länge och processen eller panen finns inte längre

### Konfiguration
`~/.config/claude-hub/config.toml`, t.ex.:
- `roots = ["~/code"]` – kataloger som skannas efter git-repon (max djup 2)
- `mouse = true` – musstöd på/av som standard
- `refresh_seconds = 2`

## TUI:n: två vyer
Gemensamt för båda vyerna:
- Flikrad överst: `[1 Claude-sessioner] [2 Repon]`. Växla med Tab, 1/2 eller klick på fliken.
- j/k eller pilar för navigering, `/` för filter, `?` för hjälp, `q` avslutar.
- Enter hoppar till vald tmux-pane (`switch-client` + `select-window` + `select-pane`).
- Automatisk uppdatering (filbevakning eller polling enligt `refresh_seconds`).
- Ska fungera som tmux-popup: `tmux display-popup -E -w 85% -h 75% claude-hub`.
- Statusrad längst ner med klickbara knappar för de vanligaste åtgärderna i aktuell vy.

### Vy 1: Claude-sessioner (agent-centrerad)
Datakälla: statusfilerna i `~/.local/state/claude-hub/`.
- En rad per session: repo, gren, status med färg och symbol, tid sedan senaste
  händelse, tmux-plats, senaste notistext.
- Sortering: `waiting` överst, sedan `working`, `idle`, sist `ended`/`stale`.
- `x` städar bort ended/stale-poster.
- `r` hoppar till sessionens repo i vy 2.

### Vy 2: Repon och terminaler (repo-centrerad)
Datakällor:
- `tmux list-panes -a -F` med session, fönsterindex och namn, pane_id,
  `pane_current_path` och `pane_current_command`.
- Git-repon från `roots` i konfigen.
- Statusfilerna från vy 1, för att koppla Claude-sessioner till repon.

Trädvy, ett repo per rad, expanderbart:
- Repo-raden: namn, gren, dirty-markering (antal ändrade filer), ahead/behind mot
  upstream, antal tmux-panes och en Claude-badge med "sämsta" status
  (t.ex. ⏳ om någon session i repot väntar på mig).
- Under repot: varje tmux-pane vars cwd ligger inuti repot (matcha mot
  `git rev-parse --show-toplevel`, så underkataloger och worktrees hamnar rätt),
  med session:fönster, kommandot som körs (nvim, zsh, claude …) och, för
  Claude-panes, dess status.
- Repon utan öppna panes visas nedtonade längst ner (döljs/visas med `h`).
- Panes vars cwd inte ligger i något repo samlas under "Övrigt".

Åtgärder på valt repo:
- Enter: hoppa till repots tmux-session, eller skapa den om den saknas
  (`tmux new-session -d -s <repo> -c <repo-rot>`) och byt sedan till den.
- `c`: starta Claude i ett nytt fönster i repots session.
- `e`: öppna nvim i ett nytt fönster i repots session.
- `t`: nytt skalfönster i repots session.
- Mellanslag eller `l`/`h` på en repo-rad: expandera/fäll ihop.

Prestanda: git-status körs asynkront och cachas (max ett anrop per repo var
5:e sekund) så att TUI:n aldrig hackar, även med många repon.

## Musstöd
- Klick på en rad väljer den. Dubbelklick (eller klick på en redan vald rad)
  motsvarar Enter.
- Klick på flikarna byter vy.
- Klick på ▸/▾ framför ett repo expanderar/fäller ihop det.
- Scrollhjul scrollar listan.
-
