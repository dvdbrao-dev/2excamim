#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

python3 agents/backtest_external_candidate.py \
  --input-jsonl "${EXT_BACKTEST_INPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --strategy-version "${EXT_BACKTEST_STRATEGY_VERSION:-oracle_lag_v1}" \
  --asset "${EXT_BACKTEST_ASSET:-BTC}" \
  --window "${EXT_BACKTEST_WINDOW:-5m}" \
  --output-report "${EXT_BACKTEST_OUTPUT_REPORT:-./reports/external_candidates/oracle_lag_v1_backtest.md}" \
  --output-jsonl "${EXT_BACKTEST_OUTPUT_JSONL:-./var/events/external_backtest.jsonl}" \
  --fee-rate-bps "${EXT_BACKTEST_FEE_BPS:-25.0}" \
  --slippage-bps "${EXT_BACKTEST_SLIPPAGE_BPS:-20.0}" \
  --latency-ms "${EXT_BACKTEST_LATENCY_MS:-800}" \
  "$@"
