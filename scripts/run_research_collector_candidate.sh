#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

python3 agents/research_collector_candidate.py \
  --input-slots-jsonl "${RESEARCH_COLLECTOR_INPUT_SLOTS:-./var/events/external_candidates.jsonl}" \
  --output-jsonl "${RESEARCH_COLLECTOR_OUTPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --assets "${RESEARCH_COLLECTOR_ASSETS:-BTC,ETH,SOL}" \
  --windows "${RESEARCH_COLLECTOR_WINDOWS:-5m,15m}" \
  --sample-count "${RESEARCH_COLLECTOR_SAMPLE_COUNT:-1}" \
  --sample-interval-ms "${RESEARCH_COLLECTOR_SAMPLE_INTERVAL_MS:-1000}" \
  --data-mode "${RESEARCH_COLLECTOR_DATA_MODE:-mock}" \
  --network-timeout-sec "${RESEARCH_COLLECTOR_NETWORK_TIMEOUT_SEC:-5}" \
  --max-retries "${RESEARCH_COLLECTOR_MAX_RETRIES:-2}" \
  "$@"
