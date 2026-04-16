#!/usr/bin/env bash
python3 scripts/repair_store.py
set -euo pipefail

cargo run -p market-watch -- --state-dir ./var/market-watch 
python3 agents/scoring_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch
python3 agents/signal_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch
python3 agents/confirmation_agent.py --store ./var/events.jsonl
python3 agents/probability_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch --max-signals 20
python3 agents/veto_agent.py --store ./var/events.jsonl --probability-floor 0.05
python3 agents/sizing_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch --bankroll 1000.0
