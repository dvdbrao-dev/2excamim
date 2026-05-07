#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

python3 agents/external_candidate_scorecard.py \
  --input-jsonl "${EXTERNAL_SCORECARD_INPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --output-jsonl "${EXTERNAL_SCORECARD_OUTPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --strategy-version "${EXTERNAL_SCORECARD_STRATEGY_VERSION:-oracle_lag_v1}" \
  --min-signals "${EXTERNAL_SCORECARD_MIN_SIGNALS:-200}" \
  --min-resolved-rounds "${EXTERNAL_SCORECARD_MIN_RESOLVED_ROUNDS:-100}" \
  --max-drawdown-usdc "${EXTERNAL_SCORECARD_MAX_DRAWDOWN_USDC:-50.0}" \
  --min-net-expectancy-bps "${EXTERNAL_SCORECARD_MIN_NET_EXPECTANCY_BPS:-3.0}" \
  "$@"
