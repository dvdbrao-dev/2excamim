#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

python3 agents/shadow_execution_simulator.py \
  --input-jsonl "${SHADOW_SIM_INPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --output-jsonl "${SHADOW_SIM_OUTPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --strategy-version "${SHADOW_SIM_STRATEGY_VERSION:-oracle_lag_v1}" \
  --fee-rate-bps "${SHADOW_SIM_FEE_RATE_BPS:-25.0}" \
  --slippage-bps "${SHADOW_SIM_SLIPPAGE_BPS:-20.0}" \
  --latency-ms "${SHADOW_SIM_LATENCY_MS:-800}" \
  --max-notional-usdc "${SHADOW_SIM_MAX_NOTIONAL_USDC:-25.0}" \
  "$@"
