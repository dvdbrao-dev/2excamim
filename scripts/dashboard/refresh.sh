#!/bin/bash
cd /root/2excamim

python3 << 'PYEOF'
import json, pathlib, collections, subprocess
from datetime import datetime, timezone

store = pathlib.Path("var/events.jsonl")
lines = store.read_text().splitlines() if store.exists() else []

eventos = collections.Counter()
llm_cost = 0.0
llm_signals = 0
decisions = []
alerts = []

for l in lines:
    if not l.strip(): continue
    try:
        d = json.loads(l)
        et = d.get("event_type","")
        eventos[et] += 1
        prov = d.get("provenance", {})
        if prov.get("actor") == "probability-agent-v1":
            notes = json.loads(prov.get("notes", "{}"))
            llm_cost += notes.get("estimated_cost_usd", 0)
            llm_signals += 1
            if et == "signal.confirmed":
                p_market = notes.get("p_market", "n/a")
                p_llm = notes.get("estimated_probability", "n/a")
                p_final = notes.get("p_final", "n/a")
                direction = notes.get("direction", "n/a")
                sid = d.get("linkage",{}).get("signal_id","")
                market_id = d.get("aggregate_key","").replace("polymarket:","")[:16]
                decisions_candidate = {
                    "market_id": market_id,
                    "p_market": p_market,
                    "p_llm": p_llm,
                    "p_final": p_final,
                    "direction": direction,
                    "occurred": d.get("occurred_at","")[:16].replace("T"," "),
                }
        if et == "decision.formed":
            payload = d.get("payload", {})
            decisions.append({
                "market": d.get("aggregate_key","").replace("polymarket:","")[:20],
                "size": round(payload.get("size_hint", 0), 2),
                "occurred": d.get("occurred_at","")[:16].replace("T"," "),
                "status": "ACTIVE"
            })
    except: pass

# Timers systemd
timers_raw = subprocess.run(
    ["systemctl", "list-timers", "--no-pager"],
    capture_output=True, text=True
).stdout

timer_names = [
    "market-watch", "dns-watchdog", "llm-health",
    "disk-guard", "excamim-daily"
]
timers = []
for line in timers_raw.splitlines():
    for name in timer_names:
        if name in line:
            parts = line.split()
            timers.append({
                "name": name,
                "next": parts[0] + " " + parts[1] if len(parts) > 1 else "n/a",
                "status": "active"
            })

# Agentes
agents = [
    {"name": "market-watch",       "lang": "Rust",   "status": "active", "role": "Data ingestion"},
    {"name": "signal-agent",       "lang": "Python", "status": "active", "role": "Signal generation"},
    {"name": "scoring-agent",      "lang": "Python", "status": "active", "role": "Market scoring"},
    {"name": "confirmation-agent", "lang": "Python", "status": "active", "role": "Signal confirmation"},
    {"name": "probability-agent",  "lang": "Python", "status": "active", "role": "LLM (gpt-4o-mini)"},
    {"name": "veto-agent",         "lang": "Python", "status": "active", "role": "Risk veto"},
    {"name": "sizing-agent",       "lang": "Python", "status": "active", "role": "Kelly sizing"},
    {"name": "exit-agent",         "lang": "Python", "status": "active", "role": "Exit triggers"},
    {"name": "telegram-agent",     "lang": "Python", "status": "active", "role": "Notifications"},
]

# Alertas
if llm_cost > 0.10:
    alerts.append({"level": "warn", "msg": f"LLM cost ${llm_cost:.3f} hoy - monitorizar créditos"})
disk = subprocess.run(["df", "-h", "/"], capture_output=True, text=True).stdout
disk_pct = 0
for line in disk.splitlines()[1:]:
    parts = line.split()
    if len(parts) >= 5:
        disk_pct = int(parts[4].replace("%",""))
        if disk_pct > 75:
            alerts.append({"level": "error", "msg": f"Disco {disk_pct}% usado"})

# Git log
git_log = subprocess.run(
    ["git", "log", "--oneline", "-8"],
    capture_output=True, text=True, cwd="/root/2excamim"
).stdout.strip().splitlines()

monthly_est = (llm_cost / max(1, datetime.now().day)) * 30

data = {
    "updated": datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC"),
    "bot": {"name": "EXCAMIM", "emoji": "⚡", "status": "online"},
    "alerts": alerts,
    "costs": {
        "today_usd": round(llm_cost, 4),
        "today_eur": round(llm_cost * 0.92, 4),
        "monthly_est_usd": round(monthly_est, 2),
        "per_signal": round(llm_cost / max(1, llm_signals), 6),
        "signals_processed": llm_signals,
    },
    "pipeline": dict(eventos),
    "decisions": decisions[-8:],
    "agents": agents,
    "timers": timers,
    "git_log": git_log,
    "system": {
        "disk_pct": disk_pct,
        "events_total": sum(eventos.values()),
        "decisions_total": eventos.get("decision.formed", 0),
        "vetos_total": eventos.get("veto.raised", 0),
        "signals_total": eventos.get("signal.generated", 0),
    }
}

pathlib.Path("scripts/dashboard/data.json").write_text(
    json.dumps(data, indent=2)
)
print("data.json updated")
PYEOF

chmod +x scripts/dashboard/refresh.sh
