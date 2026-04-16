#!/bin/bash
cd /root/2excamim

python3 << 'PYEOF'
import json, pathlib, collections, subprocess
from datetime import datetime, timezone, timedelta

BASE = pathlib.Path("/root/2excamim")
events_path = BASE / "var/events.jsonl"
snapshots_paths = [
    BASE / "var/market-watch/snapshots.jsonl",
    BASE / "var/market_snapshots.jsonl",
]


def parse_json_line(raw):
    try:
        return json.loads(raw)
    except Exception:
        return None


def parse_iso(value):
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except Exception:
        return None


def safe_float(value, default=0.0):
    try:
        return float(value)
    except Exception:
        return default


def load_market_titles(paths):
    titles = {}
    for path in paths:
        if not path.exists():
            continue
        for raw in path.read_text().splitlines():
            if not raw.strip():
                continue
            item = parse_json_line(raw)
            if not item:
                continue
            market_id = item.get("market_id")
            title = item.get("title")
            if isinstance(market_id, str) and isinstance(title, str) and title.strip():
                titles[market_id] = title
    return titles


def format_day(value):
    return value.strftime("%Y-%m-%d")


lines = events_path.read_text().splitlines() if events_path.exists() else []
market_titles = load_market_titles(snapshots_paths)

eventos = collections.Counter()
alerts = []
active_decisions = []
closed_decisions = []
vetoed_signals = []
signal_confirmations = {}
decision_rows = {}
decision_veto_rows = {}
cost_by_day = collections.Counter()
llm_cost_today = 0.0
llm_cost_week = 0.0
llm_cost_all = 0.0
llm_signals_today = 0
llm_signals_week = 0
now = datetime.now(timezone.utc)
today = now.date()
week_start = now - timedelta(days=6)

for raw in lines:
    if not raw.strip():
        continue
    try:
        d = json.loads(raw)
    except Exception:
        continue

    et = d.get("event_type", "")
    eventos[et] += 1
    occurred_at = parse_iso(d.get("occurred_at"))
    provenance = d.get("provenance") or {}
    actor = provenance.get("actor")
    payload = d.get("payload") or {}
    linkage = d.get("linkage") or {}
    aggregate_key = d.get("aggregate_key") or ""
    market_id = aggregate_key.replace("polymarket:", "") if isinstance(aggregate_key, str) else ""
    title = market_titles.get(market_id, market_id or "n/a")

    if actor == "probability-agent-v1":
        notes = provenance.get("notes", "{}")
        try:
            notes = json.loads(notes) if isinstance(notes, str) else dict(notes)
        except Exception:
            notes = {}
        cost = safe_float(notes.get("estimated_cost_usd", 0.0))
        llm_cost_all += cost
        if occurred_at is not None:
            if occurred_at.date() == today:
                llm_cost_today += cost
            if occurred_at >= week_start:
                llm_cost_week += cost
            if occurred_at.date() == today:
                llm_signals_today += 1
            if occurred_at >= week_start:
                llm_signals_week += 1
            cost_by_day[format_day(occurred_at)] += cost

        if et == "signal.confirmed":
            signal_id = linkage.get("signal_id") or payload.get("signal_id")
            signal_confirmations[signal_id] = {
                "title": title,
                "market_id": market_id,
                "p_final": safe_float(notes.get("p_final", payload.get("p_final", 0.0))),
                "direction": notes.get("direction", payload.get("direction", "n/a")),
                "p_market": safe_float(notes.get("p_market", payload.get("market_midpoint", payload.get("market_midpoint", 0.0)))),
                "p_llm": safe_float(notes.get("estimated_probability", payload.get("estimated_probability", 0.0))),
                "side": (signal_id.rsplit("-", 1)[-1] if isinstance(signal_id, str) and "-" in signal_id else payload.get("side", "n/a")),
                "occurred": occurred_at.strftime("%Y-%m-%d %H:%M") if occurred_at else d.get("occurred_at", "")[:16].replace("T", " "),
            }

        if et == "veto.raised" and payload.get("raised_by") == "probability-agent-v1":
            vetoed_signals.append({
                "title": title,
                "reason_code": payload.get("reason_code", "n/a"),
                "p_final": safe_float(payload.get("p_final", notes.get("p_final", 0.0))),
                "occurred": occurred_at.strftime("%Y-%m-%d %H:%M") if occurred_at else d.get("occurred_at", "")[:16].replace("T", " "),
            })

    if et == "decision.formed":
        decision_id = payload.get("decision_id") or linkage.get("decision_id")
        signal_id = linkage.get("signal_id") or payload.get("signal_id")
        size_hint = safe_float(payload.get("size_hint", 0.0))
        signal_info = signal_confirmations.get(signal_id, {})
        decision_rows[decision_id] = {
            "decision_id": decision_id,
            "signal_id": signal_id,
            "title": signal_info.get("title", title),
            "market": signal_info.get("market_id", market_id),
            "kelly_usd": round(size_hint, 2),
            "kelly_eur": round(size_hint * 0.92, 2),
            "p_final": safe_float(signal_info.get("p_final", 0.0)),
            "direction": signal_info.get("direction", "n/a"),
            "p_market": safe_float(signal_info.get("p_market", 0.0)),
            "p_llm": safe_float(signal_info.get("p_llm", 0.0)),
            "side": signal_info.get("side", payload.get("side", "n/a")),
            "occurred": occurred_at.strftime("%Y-%m-%d %H:%M") if occurred_at else d.get("occurred_at", "")[:16].replace("T", " "),
            "occurred_dt": occurred_at,
            "status": "ACTIVE",
            "size_hint": size_hint,
        }

    if et == "veto.raised" and payload.get("scope") == "Decision":
        target_id = payload.get("target_id")
        if isinstance(target_id, str):
            decision_veto_rows[target_id] = {
                "reason_code": payload.get("reason_code", "n/a"),
                "occurred": occurred_at,
            }

for decision in decision_rows.values():
    veto = decision_veto_rows.get(decision["decision_id"])
    if veto and (decision.get("occurred_dt") is None or (veto["occurred"] and veto["occurred"] >= decision.get("occurred_dt"))):
        closed_decisions.append({
            **{k: decision[k] for k in ["title", "kelly_usd", "kelly_eur", "occurred", "p_final", "direction", "p_market", "p_llm", "side"] if k in decision},
            "reason_code": veto["reason_code"],
            "status": "CLOSED",
        })
    else:
        active_decisions.append({k: v for k, v in decision.items() if k != "occurred_dt"})

active_decisions = active_decisions[-8:]
closed_decisions = closed_decisions[-8:]
vetoed_signals = vetoed_signals[-20:]
vetoed_signals.reverse()

bankroll_total = 1000.0
deployed_usd = round(sum(item["size_hint"] for item in active_decisions), 2)
deployed_pct = round((deployed_usd / bankroll_total) * 100 if bankroll_total else 0.0, 2)
deployed_eur = round(deployed_usd * 0.92, 2)
available_usd = round(bankroll_total - deployed_usd, 2)
available_eur = round(available_usd * 0.92, 2)

est_max_gain_usd = 0.0
for item in active_decisions:
    p_final = safe_float(item.get("p_final", 0.0))
    if p_final > 0:
        est_max_gain_usd += item["kelly_usd"] * (1.0 / p_final - 1.0)

daily_series = []
for offset in range(6, -1, -1):
    day = (today - timedelta(days=offset)).isoformat()
    daily_series.append({"date": day, "cost": round(float(cost_by_day.get(day, 0.0)), 6)})

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
if llm_cost_today > 0.10:
    alerts.append({"level": "warn", "msg": f"LLM cost ${llm_cost_today:.3f} hoy - monitorizar créditos"})
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

monthly_est = (llm_cost_today / max(1, now.day)) * 30

data = {
    "updated": now.strftime("%Y-%m-%d %H:%M UTC"),
    "bot": {"name": "EXCAMIM", "emoji": "⚡", "status": "online"},
    "alerts": alerts,
    "bankroll": {
        "bankroll_total": bankroll_total,
        "deployed_usd": deployed_usd,
        "deployed_pct": deployed_pct,
        "deployed_eur": deployed_eur,
        "available_usd": available_usd,
        "available_eur": available_eur,
    },
    "costs": {
        "today_usd": round(llm_cost_today, 4),
        "today_eur": round(llm_cost_today * 0.92, 4),
        "weekly_usd": round(llm_cost_week, 4),
        "weekly_eur": round(llm_cost_week * 0.92, 4),
        "monthly_est_usd": round(monthly_est, 2),
        "per_signal": round(llm_cost_today / max(1, llm_signals_today), 6),
        "signals_processed": llm_signals_today,
    },
    "pipeline": dict(eventos),
    "decisions": active_decisions,
    "closed_decisions": closed_decisions,
    "vetoed_signals": vetoed_signals,
    "agents": agents,
    "timers": timers,
    "git_log": git_log,
    "pnl": {
        "status": "paper_trading - awaiting resolution",
        "decisions_active": len(active_decisions),
        "decisions_closed": len(closed_decisions),
        "deployed_usd": deployed_usd,
        "est_max_gain_usd": round(est_max_gain_usd, 2),
        "est_max_loss_usd": round(sum(item["kelly_usd"] for item in active_decisions), 2),
        "daily_llm_cost": round(llm_cost_today, 4),
        "weekly_llm_cost": round(llm_cost_week, 4),
        "monthly_llm_cost": round(monthly_est, 2),
        "daily_series": daily_series,
    },
    "system": {
        "disk_pct": disk_pct,
        "events_total": sum(eventos.values()),
        "decisions_total": eventos.get("decision.formed", 0),
        "vetos_total": eventos.get("veto.raised", 0),
        "signals_total": eventos.get("signal.generated", 0),
    }
}

pathlib.Path("scripts/dashboard/data.json").write_text(json.dumps(data, indent=2))
print("data.json updated")
PYEOF

chmod +x scripts/dashboard/refresh.sh
