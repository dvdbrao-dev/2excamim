#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

python3 agents/markov_chain_signal_candidate.py \
  --input-jsonl "${MARKOV_INPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --output-jsonl "${MARKOV_OUTPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --asset "${MARKOV_ASSET:-BTC}" \
  --window "${MARKOV_WINDOW:-5m}" \
  --strategy-version "${MARKOV_STRATEGY_VERSION:-markov_chain_v1}" \
  --lookback-slots "${MARKOV_LOOKBACK_SLOTS:-2016}" \
  --min-state-samples "${MARKOV_MIN_STATE_SAMPLES:-30}" \
  --min-global-samples "${MARKOV_MIN_GLOBAL_SAMPLES:-300}" \
  --alpha "${MARKOV_ALPHA:-5}" \
  --beta "${MARKOV_BETA:-5}" \
  --min-net-edge "${MARKOV_MIN_NET_EDGE:-0.02}" \
  --cost-buffer "${MARKOV_COST_BUFFER:-0.015}" \
  --max-spread-bps "${MARKOV_MAX_SPREAD_BPS:-300}" \
  --min-depth-usdc "${MARKOV_MIN_DEPTH_USDC:-500}" \
  --min-seconds-to-expiry "${MARKOV_MIN_SECONDS_TO_EXPIRY:-45}" \
  "$@"
