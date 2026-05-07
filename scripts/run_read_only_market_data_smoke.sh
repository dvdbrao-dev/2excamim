#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

ASSETS="${ASSETS:-BTC,ETH,SOL}"
WINDOWS="${WINDOWS:-5m,15m}"
OUTPUT_JSONL="${OUTPUT_JSONL:-./var/events/read_only_smoke.jsonl}"
TIMEOUT_SEC="${TIMEOUT_SEC:-5}"
MAX_RETRIES="${MAX_RETRIES:-2}"
STRICT="${STRICT:-0}"

mkdir -p "$(dirname "$OUTPUT_JSONL")"
: > "$OUTPUT_JSONL"

echo "[read-only-smoke] output_jsonl=$OUTPUT_JSONL"

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

python3 - "$OUTPUT_JSONL" "$STRICT" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
strict = sys.argv[2] == "1"
rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]

feed = [r for r in rows if r.get("event_type") == "feed_health.checked"]
snaps = [r for r in rows if r.get("event_type") == "market_snapshot.observed"]
gaps = [r for r in rows if r.get("event_type") == "data_gap.detected"]

print(f"[read-only-smoke] feed_health.checked={len(feed)} market_snapshot.observed={len(snaps)} data_gap.detected={len(gaps)}")
if feed:
    last = feed[-1].get("payload", {})
    print("[read-only-smoke] last_feed_health=" + json.dumps({
        "ok": last.get("ok"),
        "partial": last.get("partial"),
        "reason": last.get("reason"),
        "successful_assets": last.get("successful_assets"),
        "failed_assets": last.get("failed_assets"),
        "adapter_errors": last.get("adapter_errors"),
    }, separators=(",", ":")))

if strict and not snaps:
    reason = "strict_mode_no_real_spot_snapshots"
    if feed:
        reason = json.dumps(feed[-1].get("payload", {}).get("adapter_errors", {}), separators=(",", ":"))
    print(f"[read-only-smoke] STRICT=1 failed: {reason}", file=sys.stderr)
    raise SystemExit(1)
PY

