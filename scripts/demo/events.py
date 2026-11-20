"""Write fake hook events, transcripts and plan usage for the claude-hub demo.

usage: events.py <home> <state_dir> <socket> <pay_pane> <dash_pane> <infra_pane>
"""
import json
import os
import sys
import time

home, state, socket, pay, dash, infra = sys.argv[1:]
now = int(time.time())
repos = f"{home}/repos"
events = f"{state}/events"
os.makedirs(events, exist_ok=True)

def transcript(sid, cwd, title, turns, cost, context):
    d = f"{home}/.claude/projects/" + "".join(c if c.isalnum() else "-" for c in cwd)
    os.makedirs(d, exist_ok=True)
    path = f"{d}/{sid}.jsonl"
    with open(path, "w") as f:
        for prompt, reply, tool in turns:
            f.write(json.dumps({"type": "user", "message": {"role": "user", "content": prompt}}) + "\n")
            content = [{"type": "text", "text": reply}] if reply else []
            if tool:
                content.append({"type": "tool_use", "name": tool[0], "input": tool[1]})
            f.write(json.dumps({"type": "assistant", "message": {
                "model": "claude-opus-5-5", "content": content,
                "usage": {"input_tokens": 12, "cache_read_input_tokens": context, "output_tokens": 800}}}) + "\n")
        if title:
            f.write(json.dumps({"type": "ai-title", "aiTitle": title}) + "\n")
        f.write(json.dumps({"type": "cost-state", "totalCostUSD": cost}) + "\n")
    return path

def event(sid, cwd, ago, name, pane="", sock=socket, **extra):
    e = {"session_id": sid, "cwd": cwd, "hook_event_name": name, "permission_mode": "default",
         "transcript_path": extra.pop("transcript")}
    e.update(extra)
    record = {"time": now - ago, "tmux": sock if pane else "", "tmux_pane": pane, "event": e}
    with open(f"{events}/{sid}.json", "w") as f:
        json.dump(record, f)

cwd = f"{repos}/payments-api"
t = transcript("demo-pay", cwd, "Refund webhooks with retries", [
    ("Where do we send refund webhooks today?",
     "In `src/ledger.rs`, right after the refund is booked. It sends once and drops the event "
     "if the merchant answers with an error, so a short outage loses refunds.", None),
    ("Can we retry them instead of dropping them?",
     "Yes. I suggest a retry queue with exponential backoff, and a cap so one bad merchant "
     "does not hold the queue. Do you want a cap on the delay or on the number of tries?", None),
    ("Send refund webhooks with retries and exponential backoff. Cap the delay at 10 minutes.",
     "I added a retry queue in src/webhooks/retry.rs with backoff capped at 10 minutes. "
     "Now I want to run the webhook tests to check it.",
     ("Bash", {"command": "cargo test --features webhooks", "description": "Run webhook tests"})),
], 2.84, 96000)
event("demo-pay", cwd, 40, "Notification", pay, transcript=t,
      message="Claude needs your permission to use Bash", notification_type="permission_prompt")

cwd = f"{repos}/web-dashboard"
t = transcript("demo-dash", cwd, "Fix jumping chart tooltip", [
    ("Why does the tooltip jump on the revenue chart?",
     "The tooltip is placed from the legend height. The legend wraps to two lines on narrow "
     "screens, but the height is read only on the first render.", None),
    ("The tooltip jumps on stacked charts when the legend wraps. Fix it and add a story for it.",
     "The tooltip uses the legend height from the first render. I will measure it on resize instead.",
     ("Edit", {"file_path": f"{cwd}/src/components/Chart.tsx", "old_string": "a", "new_string": "b"})),
], 1.12, 54000)
event("demo-dash", cwd, 4, "PreToolUse", dash, transcript=t,
      tool_name="Edit", tool_input={"file_path": f"{cwd}/src/components/Chart.tsx"})

cwd = f"{repos}/infra-terraform"
t = transcript("demo-infra", cwd, "Plan staging move to eu-north-1", [
    ("What runs in the staging cluster today?",
     "14 resources in eu-west-1: the EKS cluster, two node groups, RDS, Redis and the load balancers.", None),
    ("Plan the move of the staging cluster to eu-north-1. Don't apply anything.",
     "The plan is in docs/migration.md: 14 resources move, 3 need a new name, and the RDS snapshot "
     "copy takes about 40 minutes. `terraform plan` shows no changes to production. Ready when you are.",
     None),
], 0.97, 31000)
event("demo-infra", cwd, 720, "Stop", infra, transcript=t)

cwd = f"{repos}/ml-experiments"
t = transcript("demo-ml", cwd, "Tune learning rate schedule", [
    ("Try a cosine schedule with warmup and compare it with the current step schedule.",
     "I run both for 20 epochs on the small dataset first.",
     ("Bash", {"command": "python train.py --schedule cosine --epochs 20"})),
], 3.40, 128000)
event("demo-ml", cwd, 15, "PreToolUse", transcript=t,
      tool_name="Bash", tool_input={"command": "python train.py --schedule cosine --epochs 20"})

for sid, repo, title, ago in [("demo-old1", "payments-api", "Explain the ledger rounding bug", 5400),
                              ("demo-old2", "docs-site", "Update the API reference for v3", 14000)]:
    cwd = f"{repos}/{repo}"
    t = transcript(sid, cwd, title, [(title, "Done.", None)], 0.41, 20000)
    event(sid, cwd, ago, "SessionEnd", transcript=t)

with open(f"{state}/usage.json", "w") as f:
    json.dump({"five_hour": {"used": 42.0, "resets_at": now + 2 * 3600 + 900},
               "seven_day": {"used": 67.0, "resets_at": now + 3 * 86400 + 5 * 3600}}, f)
