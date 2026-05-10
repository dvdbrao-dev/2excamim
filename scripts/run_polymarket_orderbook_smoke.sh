#!/usr/bin/env bash
set -euo pipefail

export PYTHONPATH="${PYTHONPATH:-/root/2excamim}"

ASSETS="${ASSETS:-BTC,ETH,SOL}"
WINDOWS="${WINDOWS:-5m,15m}"
OUTPUT_JSONL="${OUTPUT_JSONL:-./var/events/polymarket_orderbook_smoke.jsonl}"
TIMEOUT_SEC="${TIMEOUT_SEC:-5}"
MAX_RETRIES="${MAX_RETRIES:-2}"
STRICT="${STRICT:-0}"

mkdir -p "$(dirname "$OUTPUT_JSONL")"
: > "$OUTPUT_JSONL"

echo "[polymarket-orderbook-smoke] output_jsonl=$OUTPUT_JSONL"

python3 agents/market_slot_discovery_candidate.py \
  --assets "$ASSETS" \
  --windows "$WINDOWS" \
  --lookahead-slots 0 \
  --slug-mode canonical \
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
  --polymarket-metadata-enabled \
  --polymarket-orderbook-enabled \
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

metadata_found_count = 0
orderbook_observed_count = 0
adapter_errors = {}
if feed:
    payload = feed[-1].get("payload", {})
    metadata_found_count = int(payload.get("metadata_found_count") or 0)
    orderbook_observed_count = int(payload.get("orderbook_observed_count") or 0)
    adapter_errors = payload.get("adapter_errors") or {}

slots = [r for r in rows if r.get("event_type") == "market_slot.discovered"]
slugs = [r.get("payload", {}).get("candidate_slug") for r in slots if isinstance(r.get("payload", {}).get("candidate_slug"), str)]

print(f"[polymarket-orderbook-smoke] feed_health.checked={len(feed)}")
print(f"[polymarket-orderbook-smoke] market_snapshot.observed={len(snaps)}")
print(f"[polymarket-orderbook-smoke] data_gap.detected={len(gaps)}")
print(f"[polymarket-orderbook-smoke] generated_canonical_slugs={json.dumps(slugs, separators=(',', ':'))}")
print(f"[polymarket-orderbook-smoke] metadata_found_count={metadata_found_count}")
print(f"[polymarket-orderbook-smoke] orderbook_observed_count={orderbook_observed_count}")
print(f"[polymarket-orderbook-smoke] adapter_errors={json.dumps(adapter_errors, separators=(',', ':'))}")
if feed:
    last = feed[-1].get("payload", {})
    print("[polymarket-orderbook-smoke] last_feed_health=" + json.dumps(last, separators=(",", ":")))

if strict:
    if not snaps:
        print("[polymarket-orderbook-smoke] STRICT=1 failed: no Binance spot snapshots", file=sys.stderr)
        raise SystemExit(1)
    if orderbook_observed_count == 0:
        reasons = [g.get("payload", {}).get("adapter_errors", {}) for g in gaps if g.get("payload", {}).get("gap_type") == "missing_polymarket_orderbook"]
        print(f"[polymarket-orderbook-smoke] STRICT=1 failed: expected orderbook but observed none; reasons={json.dumps(reasons, separators=(',', ':'))}", file=sys.stderr)
        raise SystemExit(1)

print(f"[polymarket-orderbook-smoke] STRICT={int(strict)} passed")
PY
