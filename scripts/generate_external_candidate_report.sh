#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

python3 agents/external_candidate_report.py \
  --input-jsonl "${EXTERNAL_REPORT_INPUT_JSONL:-./var/events/external_candidates.jsonl}" \
  --report-dir "${EXTERNAL_REPORT_DIR:-./reports/external_candidates}" \
  --strategy-version "${EXTERNAL_REPORT_STRATEGY_VERSION:-}" \
  --run-id "${EXTERNAL_REPORT_RUN_ID:-}" \
  "$@"
