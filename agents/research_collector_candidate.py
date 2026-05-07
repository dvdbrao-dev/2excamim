#!/usr/bin/env python3
"""Research snapshot collector candidate (shadow/research only)."""

from __future__ import annotations

import argparse
from collections import deque
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from adapters import BinanceSpotAdapter, PolymarketMetadataAdapter, PolymarketOrderBookAdapter
    from core.event_envelope import append_event_jsonl, build_aggregate_key, build_event, build_provenance
    from core.event_store import load_jsonl
    from core.logging import emit_json
except ModuleNotFoundError:  # pragma: no cover
    from agents.adapters import BinanceSpotAdapter, PolymarketMetadataAdapter, PolymarketOrderBookAdapter
    from agents.core.event_envelope import append_event_jsonl, build_aggregate_key, build_event, build_provenance
    from agents.core.event_store import load_jsonl
    from agents.core.logging import emit_json


AGENT_ID = "research-collector-candidate-v1"
SOURCE = "research_collector_candidate"
DEFAULT_OUTPUT = Path("./var/events/external_candidates.jsonl")
DEFAULT_INPUT_SLOTS = Path("./var/events/external_candidates.jsonl")
SUPPORTED_WINDOWS = {"5m", "15m"}
SUPPORTED_ASSETS = {"BTC", "ETH", "SOL"}


@dataclass(frozen=True)
class SlotRecord:
    asset: str
    window: str
    slot_start: str
    slot_end: str
    market_slug: str


@dataclass(frozen=True)
class MockMetadata:
    market_id: str | None
    condition_id: str | None
    token_ids: list[str] | None
    title: str | None
    active: bool | None
    resolved: bool | None
    raw_source_summary: str


def spot_delta_bps(prices: deque[float]) -> float | None:
    if len(prices) < 2:
        return None
    first = prices[0]
    last = prices[-1]
    if first <= 0:
        return None
    return ((last - first) / first) * 10_000.0


def oracle_spot_delta_bps(oracle_price: float | None, spot_price: float | None) -> float | None:
    if oracle_price is None or spot_price is None or spot_price <= 0:
        return None
    return ((oracle_price - spot_price) / spot_price) * 10_000.0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Collect research snapshots from candidate slots.")
    parser.add_argument("--input-slots-jsonl", default=str(DEFAULT_INPUT_SLOTS))
    parser.add_argument("--output-jsonl", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--assets", default="BTC,ETH,SOL")
    parser.add_argument("--windows", default="5m,15m")
    parser.add_argument("--sample-count", type=int, default=1)
    parser.add_argument("--sample-interval-ms", type=int, default=1000)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--mock", action="store_true", help="Backward-compatible alias for --data-mode mock")
    parser.add_argument("--data-mode", choices=["mock", "read_only"], default="mock")
    parser.add_argument("--network-timeout-sec", type=float, default=5.0)
    parser.add_argument("--max-retries", type=int, default=2)
    parser.add_argument("--fail-soft", action="store_true")
    parser.add_argument("--polymarket-metadata-enabled", action="store_true")
    parser.add_argument("--polymarket-orderbook-enabled", action="store_true")
    return parser.parse_args()


def _split_csv(raw: str) -> list[str]:
    return [part.strip() for part in raw.split(",") if part.strip()]


def _parse_slots(path: Path, assets: set[str], windows: set[str]) -> list[SlotRecord]:
    slots: list[SlotRecord] = []
    for record in load_jsonl(path):
        if record.get("event_type") != "market_slot.discovered":
            continue
        payload = record.get("payload") or {}
        asset = payload.get("asset")
        window = payload.get("window")
        if not isinstance(asset, str) or not isinstance(window, str):
            continue
        if asset not in assets or window not in windows:
            continue
        market_slug = payload.get("candidate_slug")
        slot_start = payload.get("slot_start")
        slot_end = payload.get("slot_end")
        if not all(isinstance(v, str) for v in (market_slug, slot_start, slot_end)):
            continue
        slots.append(SlotRecord(asset=asset, window=window, slot_start=slot_start, slot_end=slot_end, market_slug=market_slug))
    return slots


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _health_event(ok: bool, reason: str, ts: str, data_mode: str, adapter_errors: dict[str, str]) -> dict[str, Any]:
    return build_event(
        event_type="feed_health.checked",
        aggregate_key="feed:research-collector",
        payload={
            "ok": ok,
            "reason": reason,
            "source": SOURCE,
            "data_mode": data_mode,
            "adapter_errors": adapter_errors,
        },
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=["research-collector", str(ok), reason, data_mode],
        timestamp=ts,
    )


def _gap_event(reason: str, ts: str, data_mode: str, adapter_errors: dict[str, str]) -> dict[str, Any]:
    return build_event(
        event_type="data_gap.detected",
        aggregate_key="feed:research-collector",
        payload={
            "gap_type": reason,
            "source": SOURCE,
            "from": ts,
            "to": ts,
            "data_mode": data_mode,
            "adapter_errors": adapter_errors,
        },
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=["research-collector", reason, data_mode],
        timestamp=ts,
    )


def _snapshot_event(
    slot: SlotRecord,
    spot: float,
    oracle: float | None,
    metadata: MockMetadata,
    orderbook: dict[str, Any],
    latency_ms: int,
    rolling_prices: deque[float],
    data_mode: str,
    source_quality: str,
    adapter_versions: dict[str, str],
    adapter_errors: dict[str, str],
) -> dict[str, Any]:
    payload = {
        "asset": slot.asset,
        "window": slot.window,
        "slot_start": slot.slot_start,
        "slot_end": slot.slot_end,
        "market_slug": slot.market_slug,
        "spot_price": spot,
        "oracle_price": oracle,
        "metadata": {
            "market_id": metadata.market_id,
            "condition_id": metadata.condition_id,
            "token_ids": metadata.token_ids,
            "title": metadata.title,
            "active": metadata.active,
            "resolved": metadata.resolved,
            "raw_source_summary": metadata.raw_source_summary,
        },
        "orderbook": orderbook,
        "features": {
            "spot_delta_bps": spot_delta_bps(rolling_prices),
            "oracle_spot_delta_bps": oracle_spot_delta_bps(oracle, spot),
        },
        "observation_latency_ms": latency_ms,
        "source_quality": source_quality,
        "data_mode": data_mode,
        "adapter_versions": adapter_versions,
        "network_latency_ms": latency_ms,
        "adapter_errors": adapter_errors,
    }
    aggregate = build_aggregate_key("polymarket", slot.asset.lower(), slot.window)
    return build_event(
        event_type="market_snapshot.observed",
        aggregate_key=aggregate,
        payload=payload,
        provenance=build_provenance(AGENT_ID, "shadow-research"),
        unique_components=[slot.asset, slot.window, slot.slot_start, slot.market_slug, data_mode],
        timestamp=_now_iso(),
    )


def _default_metadata(slot: SlotRecord) -> MockMetadata:
    return MockMetadata(None, None, None, f"mock:{slot.market_slug}", True, False, "mock_seeded")


def _mode(args: argparse.Namespace) -> str:
    if args.mock:
        return "mock"
    return args.data_mode


def run(args: argparse.Namespace) -> dict[str, Any]:
    data_mode = _mode(args)
    assets = {a.upper() for a in _split_csv(args.assets)}
    windows = {w.lower() for w in _split_csv(args.windows)}
    bad_assets = sorted(assets - SUPPORTED_ASSETS)
    bad_windows = sorted(windows - SUPPORTED_WINDOWS)
    if bad_assets:
        raise SystemExit(f"unsupported assets: {','.join(bad_assets)}")
    if bad_windows:
        raise SystemExit(f"unsupported windows: {','.join(bad_windows)}")

    slots = _parse_slots(Path(args.input_slots_jsonl), assets, windows)
    events: list[dict[str, Any]] = []
    adapter_versions = {
        "spot": "mock_spot_adapter_v1" if data_mode == "mock" else BinanceSpotAdapter.VERSION,
        "metadata": "mock_metadata_adapter_v1" if data_mode == "mock" else PolymarketMetadataAdapter.VERSION,
        "orderbook": "mock_orderbook_adapter_v1" if data_mode == "mock" else PolymarketOrderBookAdapter.VERSION,
    }

    spot_adapter = BinanceSpotAdapter(timeout_sec=args.network_timeout_sec, max_retries=args.max_retries)
    metadata_adapter = PolymarketMetadataAdapter(timeout_sec=args.network_timeout_sec, max_retries=args.max_retries)
    orderbook_adapter = PolymarketOrderBookAdapter(
        timeout_sec=args.network_timeout_sec,
        max_retries=args.max_retries,
        enabled=bool(args.polymarket_orderbook_enabled),
    )

    if not slots:
        ts = _now_iso()
        events.append(_gap_event("missing_market_slots", ts, data_mode, {}))

    rolling: dict[tuple[str, str], deque[float]] = {}
    failures = 0

    for slot in slots:
        key = (slot.asset, slot.window)
        if key not in rolling:
            rolling[key] = deque(maxlen=max(2, args.sample_count))

        for i in range(max(1, args.sample_count)):
            adapter_errors: dict[str, str] = {}
            total_latency_ms = 0

            if data_mode == "mock":
                spot_price = {"BTC": 96000.0, "ETH": 3500.0, "SOL": 180.0}.get(slot.asset)
                metadata = _default_metadata(slot)
                orderbook = {
                    "best_bid": 0.47,
                    "best_ask": 0.49,
                    "mid_price": 0.48,
                    "spread_bps": 416.67,
                    "depth_top_n": {"n": 2, "bid": 2000.0, "ask": 1800.0},
                    "imbalance_top_n": {"n": 2, "value": 0.05263},
                }
                source_quality = "mock"
            else:
                spot = spot_adapter.observe(slot.asset)
                total_latency_ms += spot.latency_ms
                spot_price = spot.price
                if spot.error:
                    adapter_errors["spot"] = spot.error

                if args.polymarket_metadata_enabled:
                    meta = metadata_adapter.observe(slot.market_slug)
                    total_latency_ms += meta.latency_ms
                    metadata = MockMetadata(
                        meta.market_id,
                        meta.condition_id,
                        meta.token_ids,
                        meta.title,
                        meta.active,
                        meta.resolved,
                        meta.raw_source_summary,
                    )
                    if meta.error:
                        adapter_errors["metadata"] = meta.error
                        events.append(_gap_event("missing_polymarket_metadata", _now_iso(), data_mode, adapter_errors))
                else:
                    metadata = MockMetadata(None, None, None, None, None, None, "metadata_disabled")

                if args.polymarket_orderbook_enabled:
                    ob = orderbook_adapter.observe(metadata.token_ids)
                    total_latency_ms += ob.latency_ms
                    orderbook = {
                        "best_bid": ob.best_bid,
                        "best_ask": ob.best_ask,
                        "mid_price": ((ob.best_bid + ob.best_ask) / 2.0 if ob.best_bid is not None and ob.best_ask is not None else None),
                        "spread_bps": ob.spread_bps,
                        "depth_top_n": ob.depth_top_n,
                        "imbalance_top_n": {"n": 2, "value": ob.imbalance_top_n},
                    }
                    if ob.error:
                        adapter_errors["orderbook"] = ob.error
                        events.append(_gap_event("missing_polymarket_orderbook", _now_iso(), data_mode, adapter_errors))
                else:
                    orderbook = {
                        "best_bid": None,
                        "best_ask": None,
                        "mid_price": None,
                        "spread_bps": None,
                        "depth_top_n": {"n": 2, "bid": None, "ask": None},
                        "imbalance_top_n": {"n": 2, "value": None},
                    }
                    adapter_errors["orderbook"] = "orderbook_adapter_disabled"

                if args.polymarket_orderbook_enabled and "orderbook" not in adapter_errors:
                    source_quality = "read_only_orderbook"
                elif args.polymarket_metadata_enabled and "metadata" not in adapter_errors:
                    source_quality = "read_only_metadata"
                elif spot_price is not None:
                    source_quality = "read_only_spot_only"
                else:
                    source_quality = "partial"

            if spot_price is None:
                failures += 1
                events.append(_gap_event(f"missing_spot_price:{slot.asset}", _now_iso(), data_mode, adapter_errors))
                continue

            rolling[key].append(float(spot_price) + (i * 0.01))
            events.append(
                _snapshot_event(
                    slot=slot,
                    spot=float(spot_price) + (i * 0.01),
                    oracle=None,
                    metadata=metadata,
                    orderbook=orderbook,
                    latency_ms=max(total_latency_ms, args.sample_interval_ms),
                    rolling_prices=rolling[key],
                    data_mode=data_mode,
                    source_quality=source_quality,
                    adapter_versions=adapter_versions,
                    adapter_errors=adapter_errors,
                )
            )

    ts = _now_iso()
    ok = failures == 0
    reason = "ok" if ok else "spot_failures_detected"
    events.insert(0, _health_event(ok, reason, ts, data_mode, {}))

    output = Path(args.output_jsonl)
    persisted = 0
    for event in events:
        if append_event_jsonl(output, event, dry_run=args.dry_run):
            persisted += 1

    if failures > 0 and not args.fail_soft:
        raise SystemExit("read_only collection encountered spot failures; use --fail-soft to continue")

    return {
        "actor": AGENT_ID,
        "source": SOURCE,
        "mock": data_mode == "mock",
        "data_mode": data_mode,
        "dry_run": bool(args.dry_run),
        "input_slots_jsonl": str(args.input_slots_jsonl),
        "output_jsonl": str(output),
        "slots_read": len(slots),
        "events_generated": len(events),
        "events_persisted": persisted,
        "sample_count": args.sample_count,
        "network_timeout_sec": args.network_timeout_sec,
        "max_retries": args.max_retries,
    }


def main() -> int:
    args = parse_args()
    summary = run(args)
    emit_json(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
