#!/usr/bin/env bash
set -euo pipefail

cd /root/2excamim || exit 1

STORE="./var/events.jsonl"
FAIL_DIR="./var/.failures"

python3 - <<'PY'
import json
from datetime import datetime, timezone
from pathlib import Path

store = Path("./var/events.jsonl")
fail_dir = Path("./var/.failures")
agents = ["confirmation", "sizing", "exit", "shadow_live", "drawdown_guard", "veto", "telegram"]
agent_to_type = {
    "confirmation": "signal.confirmed",
    "sizing": "decision.formed",
    "exit": "veto.raised",
    "shadow_live": "shadow.fill_simulated",
    "drawdown_guard": "kill_switch.triggered",
    "veto": "veto.raised",
    "telegram": "notification.sent",
}

def parse_ts(value):
    if not isinstance(value, str):
        return None
    if value.endswith("Z"):
        value = value[:-1] + "+00:00"
    try:
        return datetime.fromisoformat(value).astimezone(timezone.utc)
    except ValueError:
        return None

def event_ts(ev):
    for key in ("timestamp", "ts", "event_ts", "created_at"):
        dt = parse_ts(ev.get(key))
        if dt:
            return dt
    return None

now = datetime.now(timezone.utc)
last_by_type = {}
if store.exists():
    with store.open("r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            ev_type = ev.get("event_type")
            ts = event_ts(ev)
            if not ev_type or not ts:
                continue
            prev = last_by_type.get(ev_type)
            if prev is None or ts > prev:
                last_by_type[ev_type] = ts

print("=== Pipeline Health ===")
for agent in agents:
    ev_type = agent_to_type[agent]
    last_ts = last_by_type.get(ev_type)
    last_ts_s = last_ts.isoformat().replace("+00:00", "Z") if last_ts else "N/A"
    count_path = fail_dir / f"{agent}.count"
    last_error_path = fail_dir / f"{agent}.last_error"
    fail_count = "0"
    if count_path.exists():
        try:
            fail_count = count_path.read_text(encoding="utf-8").strip() or "0"
        except OSError:
            fail_count = "ERR"

    print(f"[{agent}] last_event={last_ts_s} failures={fail_count}")
    if last_error_path.exists():
        try:
            content = last_error_path.read_text(encoding="utf-8", errors="replace").strip()
        except OSError:
            content = "<read_error>"
        print("last_error:")
        print(content)

last_decision = last_by_type.get("decision.formed")
if last_decision is None:
    print("decision.formed age_hours=INF")
    print("ALERT: decision.formed age > 6h")
else:
    age_hours = (now - last_decision).total_seconds() / 3600.0
    print(f"decision.formed age_hours={age_hours:.2f}")
    if age_hours > 6.0:
        print("ALERT: decision.formed age > 6h")
PY
