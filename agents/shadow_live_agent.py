#!/usr/bin/env python3
"""Shadow Live Agent.

Reads decision.formed events and simulates realistic fills using the observed
market snapshot at the next available tick. Emits shadow.fill.received events
(aggregate_key prefixed with 'shadow:') to the event store.

Never touches real money. Distinguishable by aggregate_key prefix.
"""
from __future__ import annotations

import argparse
import json
import sys
import uuid
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
AGENTS_DIR = ROOT / "agents"
for _p in [str(ROOT), str(AGENTS_DIR)]:
    if _p not in sys.path:
        sys.path.insert(0, _p)

from core.event_store import (
    append_event_idempotent,
    default_checkpoint_path,
    load_checkpoint,
    load_jsonl,
    parse_timestamp,
    read_jsonl_since,
    save_checkpoint,
)
from core.logging import emit_json
from core.time import execution_run_id, utc_now_rfc3339

AGENT_ID = "shadow-live-agent-v1"
PRODUCED_BY = "runtime.agent.shadow_live"
DEFAULT_STORE = Path("./var/events.jsonl")
DEFAULT_WATCH_DIR = Path("./var/market-watch")
DEFAULT_POLYMARKET_SLIPPAGE_BPS = 30.0
DEFAULT_CRYPTO_SLIPPAGE_BPS = 5.0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Shadow live fill simulator.")
    parser.add_argument("--store", default=str(DEFAULT_STORE))
    parser.add_argument("--watch-dir", default=str(DEFAULT_WATCH_DIR), dest="watch_dir")
    parser.add_argument("--polymarket-slippage-bps", type=float,
                        default=DEFAULT_POLYMARKET_SLIPPAGE_BPS, dest="polymarket_slippage_bps")
    parser.add_argument("--crypto-slippage-bps", type=float,
                        default=DEFAULT_CRYPTO_SLIPPAGE_BPS, dest="crypto_slippage_bps")
    parser.add_argument("--config", default="config/risk_limits.yaml")
    return parser.parse_args()


def _load_config(config_path: Path) -> dict[str, Any]:
    """Load risk_limits.yaml using the same minimal parser as the gauntlet."""
    from scripts.edge_validation_gauntlet import load_yaml_simple
    if config_path.exists():
        return load_yaml_simple(config_path)
    return {}


def _latest_snapshots(watch_dir: Path) -> dict[str, dict[str, Any]]:
    """Load latest market snapshots keyed by market_id."""
    snapshots: dict[str, dict[str, Any]] = {}
    path = watch_dir / "snapshots.jsonl"
    if not path.exists():
        return snapshots
    for snap in load_jsonl(path):
        mid = snap.get("market_id")
        if not isinstance(mid, str):
            continue
        ts = parse_timestamp(snap.get("observed_at"))
        prev = snapshots.get(mid)
        if prev is None or (ts and parse_timestamp(prev.get("observed_at", "")) < ts):
            snapshots[mid] = snap
    return snapshots


def _market_midpoint(snapshot: dict[str, Any]) -> float | None:
    best_bid = snapshot.get("best_bid")
    best_ask = snapshot.get("best_ask")
    if best_bid is not None and best_ask is not None:
        try:
            mid = (float(best_bid) + float(best_ask)) / 2.0
            if 0.0 < mid < 1.0:
                return mid
        except (TypeError, ValueError):
            pass
    return None


def _is_crypto_instrument(instrument: str) -> bool:
    return not instrument.startswith("0x") and not instrument.startswith("polymarket:")


def _apply_slippage(
    price: float,
    side: str,
    slippage_bps: float,
) -> float:
    """Apply slippage to execution price. Adverse direction always."""
    slip = slippage_bps / 10_000.0
    if side.upper() == "YES":
        # Buying YES → price moves up (worse for buyer)
        return min(1.0, price * (1.0 + slip))
    else:
        # Buying NO → price moves up (worse for buyer)
        return min(1.0, price * (1.0 + slip))


def collect_unprocessed_decisions(
    events: list[dict[str, Any]],
    processed_decision_ids: set[str],
) -> list[dict[str, Any]]:
    """Collect decision.formed events not yet shadow-filled."""
    shadow_filled: set[str] = set()
    decisions: list[dict[str, Any]] = []

    for event in events:
        et = event.get("event_type")
        payload = event.get("payload") or {}
        linkage = event.get("linkage") or {}

        if et in ("fill.received", "shadow.fill.received"):
            agg = event.get("aggregate_key", "")
            if isinstance(agg, str) and agg.startswith("shadow:"):
                dec_id = payload.get("decision_id") or linkage.get("decision_id")
                if isinstance(dec_id, str):
                    shadow_filled.add(dec_id)

        if et == "decision.formed":
            dec_id = payload.get("decision_id") or linkage.get("decision_id")
            if isinstance(dec_id, str) and dec_id not in processed_decision_ids:
                decisions.append(event)

    return [d for d in decisions
            if (d.get("payload") or {}).get("decision_id") not in shadow_filled]


def build_shadow_fill_event(
    decision_event: dict[str, Any],
    fill_price: float,
    quantity: float,
    run_id: str,
    slippage_bps: float,
) -> dict[str, Any]:
    payload = decision_event.get("payload") or {}
    linkage = decision_event.get("linkage") or {}
    decision_id = payload.get("decision_id") or linkage.get("decision_id") or "unknown"
    instrument = payload.get("instrument") or decision_event.get("aggregate_key") or ""
    side = payload.get("side", "YES")
    size_hint = float(payload.get("size_hint", 0.0))
    fill_id = f"shadow-fill-{decision_id}"

    orig_agg = decision_event.get("aggregate_key", instrument)
    shadow_agg = f"shadow:{orig_agg}" if not orig_agg.startswith("shadow:") else orig_agg

    return {
        "event_id": str(uuid.uuid4()),
        "event_type": "shadow.fill.received",
        "schema_version": "v1",
        "occurred_at": utc_now_rfc3339(),
        "produced_by": PRODUCED_BY,
        "idempotency_key": f"shadow.fill.received:v1:{decision_id}:{AGENT_ID}",
        "aggregate_key": shadow_agg,
        "linkage": {
            "hypothesis_id": linkage.get("hypothesis_id"),
            "signal_id": linkage.get("signal_id"),
            "decision_id": decision_id,
            "order_id": fill_id,
            "position_id": None,
            "parent_event_id": decision_event.get("event_id"),
            "correlation_id": linkage.get("correlation_id"),
        },
        "provenance": {
            "source_kind": "Runtime",
            "source_ref": "shadow_live",
            "producer_run_id": run_id,
            "actor": AGENT_ID,
            "trace_id": f"{run_id}:{decision_id}",
            "notes": (
                f"shadow fill: price={fill_price:.6f} qty={quantity:.6f} "
                f"slippage_bps={slippage_bps:.1f} size_hint={size_hint:.2f}"
            ),
        },
        "payload": {
            "instrument": instrument,
            "fill_id": fill_id,
            "order_id": fill_id,
            "decision_id": decision_id,
            "side": side,
            "price": round(fill_price, 6),
            "quantity": round(quantity, 6),
            "executed_at": utc_now_rfc3339(),
            "slippage_bps": slippage_bps,
            "shadow": True,
        },
    }


def main() -> int:
    args = parse_args()
    store_path = Path(args.store)
    watch_dir = Path(args.watch_dir)
    config_path = ROOT / args.config

    cfg = _load_config(config_path)
    shadow_cfg = cfg.get("shadow_live", {})
    if not shadow_cfg.get("enabled", True):
        emit_json({"actor": AGENT_ID, "status": "disabled"})
        return 0

    poly_slippage = float(shadow_cfg.get("polymarket_slippage_bps", args.polymarket_slippage_bps))
    crypto_slippage = float(shadow_cfg.get("crypto_slippage_bps", args.crypto_slippage_bps))

    run_id = execution_run_id(AGENT_ID)
    checkpoint_path = default_checkpoint_path(store_path, AGENT_ID)
    checkpoint = load_checkpoint(checkpoint_path)
    offset = int(checkpoint.get("offset", 0))
    processed: set[str] = set(checkpoint.get("processed_decision_ids", []))

    events, next_offset = read_jsonl_since(store_path, offset)
    snapshots = _latest_snapshots(watch_dir)

    pending = collect_unprocessed_decisions(events, processed)
    filled = skipped = 0

    for decision_event in pending:
        payload = decision_event.get("payload") or {}
        decision_id = payload.get("decision_id") or (decision_event.get("linkage") or {}).get("decision_id")
        if not isinstance(decision_id, str):
            skipped += 1
            continue

        instrument = payload.get("instrument") or decision_event.get("aggregate_key") or ""
        side = payload.get("side", "YES")
        size_hint = float(payload.get("size_hint", 0.0))

        if size_hint <= 0:
            skipped += 1
            processed.add(decision_id)
            continue

        # Determine fill price
        is_crypto = _is_crypto_instrument(instrument)
        slippage = crypto_slippage if is_crypto else poly_slippage

        # Find snapshot for this market
        market_id = instrument.replace("polymarket:", "").lstrip("0x")
        snap = snapshots.get(instrument) or snapshots.get(f"polymarket:{instrument}")
        mid = _market_midpoint(snap) if snap else None

        if mid is None:
            # No snapshot → skip (fail-closed: don't simulate without price data)
            skipped += 1
            processed.add(decision_id)
            emit_json({
                "actor": AGENT_ID,
                "decision_id": decision_id,
                "status": "skipped",
                "reason": "no_snapshot",
            })
            continue

        fill_price = _apply_slippage(mid, side, slippage)
        quantity = size_hint / fill_price if fill_price > 0 else 0.0

        shadow_event = build_shadow_fill_event(
            decision_event, fill_price, quantity, run_id, slippage
        )
        persisted = append_event_idempotent(store_path, shadow_event)
        if persisted:
            filled += 1
        processed.add(decision_id)

    safe_offset = store_path.stat().st_size if store_path.exists() else next_offset
    save_checkpoint(checkpoint_path, {
        "offset": safe_offset,
        "processed_decision_ids": sorted(processed),
    })

    emit_json({
        "actor": AGENT_ID,
        "run_id": run_id,
        "shadow_fills_written": filled,
        "skipped": skipped,
        "pending_decisions": len(pending),
    })
    return 0


if __name__ == "__main__":
    sys.exit(main())
