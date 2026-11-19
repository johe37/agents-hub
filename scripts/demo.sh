#!/usr/bin/env bash
# Start a claude-hub with made-up workspaces, agents and merge requests, for screenshots.
# It runs on its own tmux server with its own HOME, so your real hub is not touched.
set -euo pipefail

usage() {
    echo "usage: $0 [start | stop]" >&2
    exit 1
}

KIT=$(cd "$(dirname "$0")/demo" && pwd)
ROOT=$(cd "$KIT/../.." && pwd)
D=${XDG_CACHE_HOME:-$HOME/.cache}/claude-hub-demo
# Socket paths are limited to about 100 characters, so keep the server out of $D.
export TMUX_TMPDIR=${XDG_RUNTIME_DIR:-/tmp}/claude-hub-demo
OUTER_TMUX=${TMUX:-}
ATTACH="TMUX_TMPDIR=$TMUX_TMPDIR tmux -L claude-hub attach -t _hub"

tmux -L claude-hub kill-server 2>/dev/null || true
case ${1:-start} in
stop)
    rm -rf "$D" "$TMUX_TMPDIR"
    exit 0
    ;;
start) ;;
*) usage ;;
esac

cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml"
HUB=$ROOT/target/release/claude-hub

rm -rf "$D" "$TMUX_TMPDIR"
mkdir -p "$D/home" "$D/bin" "$D/remotes" "$TMUX_TMPDIR"

export HOME=$D/home
unset TMUX TMUX_PANE XDG_STATE_HOME XDG_CACHE_HOME XDG_CONFIG_HOME
export PATH=$D/bin:$PATH
export SHELL=/bin/zsh
cp "$KIT/glab" "$KIT/claude" "$D/bin/"

cat > "$HOME/.zshrc" <<'EOF'
PROMPT='%F{cyan}%~%f %F{magenta}❯%f '
alias ls='ls --color=auto'
EOF
cat > "$HOME/.gitconfig" <<'EOF'
[user]
	name = Demo
	email = demo@example.com
[init]
	defaultBranch = master
EOF
mkdir -p "$HOME/.config/claude-hub"
cat > "$HOME/.config/claude-hub/config.toml" <<'EOF'
roots = ["~/repos"]
prewarm = false
prewarm_claude = false
EOF

R=$HOME/repos
mkrepo() { # name branch files...
    local name=$1 branch=$2
    shift 2
    git init -q --bare "$D/remotes/$name.git"
    git clone -q "$D/remotes/$name.git" "$R/$name" 2>/dev/null
    cd "$R/$name"
    for f in "$@"; do
        mkdir -p "$(dirname "$f")"
        echo "// $f" > "$f"
    done
    echo "# $name" > README.md
    git add -A && git commit -qm "Initial commit" && git push -q origin master
    [ "$branch" = master ] || git checkout -qb "$branch"
}

mkrepo payments-api feature/refund-webhooks src/webhooks/retry.rs src/ledger.rs tests/refund_webhooks.rs Cargo.toml
git commit -qm "Add retry queue for webhooks" --allow-empty && git push -qu origin HEAD
git commit -qm "Cap retry delay" --allow-empty
git commit -qm "Log failed deliveries" --allow-empty
echo change >> src/webhooks/retry.rs
echo change >> tests/refund_webhooks.rs
echo new > src/webhooks/backoff.rs

mkrepo web-dashboard fix/chart-tooltips src/components/Chart.tsx src/components/Legend.tsx package.json
git commit -qm "Measure legend on resize" --allow-empty && git push -qu origin HEAD
echo change >> src/components/Chart.tsx

mkrepo infra-terraform master staging/main.tf prod/main.tf docs/migration.md
git clone -q "$D/remotes/infra-terraform.git" "$D/other"
(cd "$D/other" && git commit -qm "Bump provider" --allow-empty && git commit -qm "Tag staging" --allow-empty && git push -q)
rm -rf "$D/other"
git fetch -q

mkrepo docs-site master content/api.md content/guide.md
mkrepo ml-experiments exp/cosine-schedule train.py
mkrepo billing-service master src/vat/no.rs
mkrepo mobile-app master app/account.ts

cd "$HOME"
# Starts the tmux server and the hub, then fails to attach without a terminal; that is fine.
for ws in payments-api web-dashboard infra-terraform docs-site; do
    "$HUB" "$R/$ws" >/dev/null 2>&1 || true
done
t() { tmux -L claude-hub "$@"; }
t new-window -d -t =payments-api: -n claude -c "$R/payments-api" "claude payments"
t new-window -d -t =payments-api: -n nvim -c "$R/payments-api" "nvim src/webhooks/retry.rs"
t new-window -d -t =web-dashboard: -n claude -c "$R/web-dashboard" "claude dashboard"
t new-window -d -t =infra-terraform: -n claude -c "$R/infra-terraform" "claude infra"
pane() { t display-message -p -t "=$1:claude" '#{pane_id}'; }

python3 "$KIT/events.py" "$HOME" "$HOME/.local/state/claude-hub" "$TMUX_TMPDIR/tmux-$(id -u)/claude-hub" \
    "$(pane payments-api)" "$(pane web-dashboard)" "$(pane infra-terraform)"

# The hub previews a terminal from what it prints after the hub starts piping it.
sleep 4
t send-keys -t =payments-api:shell 'clear; git --no-pager status -sb; git --no-pager log --oneline -4' Enter
t send-keys -t =web-dashboard:shell "clear; printf '\n  \e[32mVITE v6.2.0\e[0m  ready in 412 ms\n\n  ➜  Local:   http://localhost:5173/\n  ➜  Network: use --host to expose\n\n  \e[2m14:02:11\e[0m \e[32mhmr update\e[0m /src/components/Chart.tsx\n'" Enter
t send-keys -t =infra-terraform:shell 'clear; git --no-pager status -sb; ls staging' Enter
t send-keys -t =docs-site:shell 'clear; ls -la content' Enter

if [ -t 1 ] && [ -z "$OUTER_TMUX" ]; then
    exec env TMUX_TMPDIR="$TMUX_TMPDIR" tmux -L claude-hub attach -t _hub
fi
echo "Demo ready. Attach outside tmux with:"
echo "  $ATTACH"
