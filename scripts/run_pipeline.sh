#!/usr/bin/env bash
python3 scripts/repair_store.py
set -euo pipefail

timeout 60 cargo run -p market-watch -- --state-dir ./var/market-watch 
# python3 agents/crypto_price_agent.py --store ./var/events.jsonl --threshold 2.0 2>&1
# python3 agents/crypto_market_matcher.py --store ./var/events.jsonl --watch-dir ./var/market-watch 2>&1
if [[ "${ENABLE_CRYPTO_STRATEGIES:-0}" == "1" ]]; then
  python3 agents/crypto_adx_ema_pullback_agent.py --store ./var/events.jsonl --cache-dir ./var/crypto_ohlcv || echo "WARN: crypto_adx_ema_pullback_agent failed, continuing"
  python3 agents/crypto_volatility_breakout_agent.py --store ./var/events.jsonl --cache-dir ./var/crypto_ohlcv || echo "WARN: crypto_volatility_breakout_agent failed, continuing"
  python3 agents/crypto_strategy_scorecard_agent.py --store ./var/events.jsonl --json || echo "WARN: crypto_strategy_scorecard_agent failed, continuing"
fi
python3 agents/scoring_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch
python3 agents/signal_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch
python3 agents/confirmation_agent.py --store ./var/events.jsonl
python3 agents/probability_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch --max-signals 20
python3 agents/veto_agent.py --store ./var/events.jsonl --probability-floor 0.05
python3 agents/sizing_agent.py --store ./var/events.jsonl --watch-dir ./var/market-watch --bankroll 1000.0
python3 agents/telegram_agent.py --store ./var/events.jsonl 2>&1
# Live Gateway — descomentar el 22 abril post-migración V2
# python3 agents/live_gateway.py --store ./var/events.jsonl
