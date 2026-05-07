#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

ASSETS="${ASSETS:-BTC,ETH,SOL}"
WINDOWS="${WINDOWS:-5m,15m}"
OUTPUT_JSONL="${OUTPUT_JSONL:-./var/events/read_only_smoke.jsonl}"
TIMEOUT_SEC="${TIMEOUT_SEC:-5}"
MAX_RETRIES="${MAX_RETRIES:-2}"

mkdir -p "$(dirname "$OUTPUT_JSONL")"

python3 agents/market_slot_discovery_candidate.py \
  --assets "$ASSETS" \
  --windows "$WINDOWS" \
  --lookahead-slots 0 \
  --output-jsonl "$OUTPUT_JSONL"

python3 agents/research_collector_candidate.py \
  --input-slots-jsonl "$OUTPUT_JSONL" \
  --output-jsonl "$OUTPUT_JSONL" \
  --assets "$ASSETS" \
  --windows "$WINDOWS" \
  --sample-count 1 \
  --sample-interval-ms 1000 \
  --data-mode read_only \
  --network-timeout-sec "$TIMEOUT_SEC" \
  --max-retries "$MAX_RETRIES" \
  --fail-soft

echo "[read-only-smoke] wrote events to $OUTPUT_JSONL"
