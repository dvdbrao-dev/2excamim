#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

python3 agents/market_slot_discovery_candidate.py \
  --assets "${SLOT_DISCOVERY_ASSETS:-BTC,ETH,SOL}" \
  --windows "${SLOT_DISCOVERY_WINDOWS:-5m,15m}" \
  --lookahead-slots "${SLOT_DISCOVERY_LOOKAHEAD_SLOTS:-2}" \
  --output-jsonl "${SLOT_DISCOVERY_OUTPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  "$@"
