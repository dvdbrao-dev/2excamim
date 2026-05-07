#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

python3 agents/oracle_lag_signal_candidate.py \
  --input-jsonl "${ORACLE_LAG_INPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --output-jsonl "${ORACLE_LAG_OUTPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --asset "${ORACLE_LAG_ASSET:-BTC}" \
  --window "${ORACLE_LAG_WINDOW:-5m}" \
  --strategy-version "${ORACLE_LAG_STRATEGY_VERSION:-oracle_lag_v1}" \
  --min-spot-move-bps "${ORACLE_LAG_MIN_SPOT_MOVE_BPS:-8.0}" \
  --max-market-price "${ORACLE_LAG_MAX_MARKET_PRICE:-0.92}" \
  --min-edge-bps "${ORACLE_LAG_MIN_EDGE_BPS:-5.0}" \
  "$@"
