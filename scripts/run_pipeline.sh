#!/usr/bin/env bash
set -euo pipefail

python3 - <<'EOF'
import json, pathlib
for fname in ["var/market-watch/snapshots.jsonl", "var/market_snapshots.jsonl"]:
    p = pathlib.Path(fname)
    if not p.exists():
        continue
    valid = []
    for line in p.read_text().splitlines():
        if not line.strip():
            continue
        try:
            json.loads(line)
            valid.append(line)
        except Exception:
            pass
    p.write_text("\n".join(valid) + "\n")
    print(f"cleaned {fname}: {len(valid)} valid lines")
EOF

cargo run -p market-watch -- --state-dir ./var/market-watch 
python3 agents/scoring_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch
python3 agents/signal_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch
python3 agents/confirmation_agent.py --store ./var/events.jsonl
python3 agents/probability_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch --max-signals 20
python3 agents/veto_agent.py --store ./var/events.jsonl --probability-floor 0.05
python3 agents/sizing_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch --bankroll 1000.0
