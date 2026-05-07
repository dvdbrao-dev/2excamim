#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

EXTERNAL_EDGE_MOCK_MODE="${EXTERNAL_EDGE_MOCK_MODE:-1}"
EXTERNAL_EDGE_OUTPUT_JSONL="${EXTERNAL_EDGE_OUTPUT_JSONL:-./var/events/external_candidates.jsonl}"

failures=0

run_stage() {
  local name="$1"
  shift

  if "$@"; then
    echo "[external-edge] ok: ${name}"
    return 0
  fi

  failures=$((failures + 1))
  echo "[external-edge] ERROR: ${name} failed (continuing with remaining stages)" >&2
  return 0
}

slot_args=()
collector_args=()
signal_args=()
shadow_args=()
scorecard_args=()

if [[ "${EXTERNAL_EDGE_MOCK_MODE}" == "1" ]]; then
  slot_args+=(--dry-run)
  collector_args+=(--mock --dry-run)
  signal_args+=(--dry-run)
  shadow_args+=(--dry-run)
  scorecard_args+=(--dry-run)
fi

run_stage "slot_discovery_candidate" \
  python3 agents/market_slot_discovery_candidate.py \
    --output-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    "${slot_args[@]}"

run_stage "research_collector_candidate" \
  python3 agents/research_collector_candidate.py \
    --input-slots-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    --output-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    "${collector_args[@]}"

run_stage "oracle_lag_signal_candidate" \
  python3 agents/oracle_lag_signal_candidate.py \
    --input-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    --output-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    "${signal_args[@]}"

run_stage "shadow_execution_simulator" \
  python3 agents/shadow_execution_simulator.py \
    --input-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    --output-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    "${shadow_args[@]}"

run_stage "external_candidate_scorecard" \
  python3 agents/external_candidate_scorecard.py \
    --input-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    --output-jsonl "${EXTERNAL_EDGE_OUTPUT_JSONL}" \
    "${scorecard_args[@]}"

if (( failures > 0 )); then
  echo "[external-edge] completed with ${failures} stage failure(s). See logs above." >&2
  exit 1
fi

echo "[external-edge] completed successfully"
